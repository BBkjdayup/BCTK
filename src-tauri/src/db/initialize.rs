use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    Connection, Row, SqliteConnection, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous},
};

use super::{DatabaseError, DatabasePaths, DatabaseResult};

/// Big-endian ASCII "ZTKU". Existing non-empty files must already carry this
/// identifier. In particular, an application_id of zero is never claimed for
/// an existing database, even if it contains familiar-looking table names.
pub const APPLICATION_ID: i64 = 1_515_473_749;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct Database {
    pool: SqlitePool,
    paths: DatabasePaths,
}

#[derive(Debug)]
struct MigrationFiles {
    candidate: PathBuf,
    backup: PathBuf,
    failed_candidate: PathBuf,
}

impl MigrationFiles {
    fn new(paths: &DatabasePaths) -> DatabaseResult<Self> {
        let token = format!("{}-{}", unix_millis()?, uuid::Uuid::now_v7().simple());
        let database_dir = paths.database_file().parent().ok_or_else(|| {
            DatabaseError::MigrationSwitch(
                "the database file does not have a parent directory".to_owned(),
            )
        })?;

        Ok(Self {
            candidate: database_dir.join(format!(".zhitiku-migration-{token}.sqlite3")),
            backup: paths
                .backup_dir()
                .join(format!("zhitiku-pre-migration-{token}.sqlite3")),
            failed_candidate: paths
                .backup_dir()
                .join(format!("zhitiku-failed-migration-{token}.sqlite3")),
        })
    }
}

impl Database {
    pub fn current_schema_version() -> i64 {
        MIGRATOR
            .iter()
            .filter(|migration| migration.migration_type.is_up_migration())
            .map(|migration| migration.version)
            .max()
            .unwrap_or(0)
    }

    pub async fn open(data_root: impl Into<PathBuf>, app_version: &str) -> DatabaseResult<Self> {
        let paths = DatabasePaths::new(data_root);
        let existed_nonempty = file_is_nonempty(paths.database_file())?;

        paths.prepare()?;

        if existed_nonempty {
            Self::upgrade_existing_database_safely(&paths, app_version).await?;
        }

        let pool = connect_pool(paths.database_file(), true, 4).await?;
        let bootstrap_result = if existed_nonempty {
            Self::bootstrap_existing(&pool).await
        } else {
            Self::bootstrap_new(&pool, app_version).await
        };

        if let Err(error) = bootstrap_result {
            pool.close().await;
            return Err(error);
        }

        Ok(Self { pool, paths })
    }

    /// Initialization may migrate the live file because it did not contain any
    /// user data before this call. Existing non-empty files take the separate
    /// copy-migrate-verify-switch path below.
    async fn bootstrap_new(pool: &SqlitePool, app_version: &str) -> DatabaseResult<()> {
        Self::verify_or_initialize_application_id(pool, true).await?;
        Self::enable_wal(pool).await?;
        MIGRATOR.run(pool).await?;
        Self::ensure_database_identity(pool, app_version).await?;
        Self::verify_integrity_pool(pool).await
    }

    async fn bootstrap_existing(pool: &SqlitePool) -> DatabaseResult<()> {
        Self::verify_or_initialize_application_id(pool, false).await?;

        if !Self::schema_is_current_pool(pool).await? {
            return Err(DatabaseError::MigrationSwitch(
                "the live database is still pending a migration after the safe upgrade stage"
                    .to_owned(),
            ));
        }

        Self::enable_wal(pool).await?;
        Self::verify_integrity_pool(pool).await
    }

    /// Existing databases are never migrated in place. A consistent SQLite
    /// snapshot is migrated and verified first. Only then is it switched into
    /// place, while the untouched old database is retained in backup/.
    async fn upgrade_existing_database_safely(
        paths: &DatabasePaths,
        app_version: &str,
    ) -> DatabaseResult<()> {
        let mut source = connect_connection(paths.database_file(), false).await?;

        Self::verify_application_id_connection(&mut source, false).await?;
        Self::verify_integrity_connection(&mut source).await?;

        if Self::schema_is_current_connection(&mut source).await? {
            source.close().await?;
            return Ok(());
        }

        // The exclusive locking mode keeps another SQLite connection from
        // changing the source while VACUUM INTO creates the snapshot.
        Self::hold_exclusive_lock(&mut source).await?;
        Self::verify_integrity_connection(&mut source).await?;

        let files = MigrationFiles::new(paths)?;
        if files.candidate.exists() || files.backup.exists() || files.failed_candidate.exists() {
            return Err(DatabaseError::MigrationSwitch(
                "a unique migration working path unexpectedly already exists".to_owned(),
            ));
        }

        if let Err(error) = Self::create_consistent_snapshot(&mut source, &files.candidate).await {
            let _ = remove_owned_work_file(&files.candidate);
            return Err(error);
        }

        if let Err(error) = Self::migrate_and_verify_candidate(&files.candidate, app_version).await
        {
            let _ = remove_owned_work_file(&files.candidate);
            return Err(error);
        }

        // A retained old database must be self-contained. Flush any WAL pages
        // and return the source to DELETE mode before its main file is backed up.
        if let Err(error) = Self::prepare_source_for_switch(&mut source).await {
            let _ = remove_owned_work_file(&files.candidate);
            return Err(error);
        }
        source.close().await?;
        sync_file(&files.candidate)?;

        if let Err(error) =
            switch_database_files(paths.database_file(), &files.candidate, &files.backup)
        {
            let recovery = recover_failed_switch(
                paths.database_file(),
                &files.backup,
                &files.failed_candidate,
            );
            return match recovery {
                Ok(()) => Err(DatabaseError::MigrationSwitch(format!(
                    "atomic replacement failed; the original database remains active: {error}"
                ))),
                Err(recovery_error) => Err(DatabaseError::MigrationSwitch(format!(
                    "atomic replacement failed ({error}); automatic recovery also failed \
                     ({recovery_error}). The pre-migration database is retained at {}",
                    files.backup.display()
                ))),
            };
        }

        let switched_validation =
            sync_file(paths.database_file()).and_then(|()| sync_file(&files.backup));
        let switched_validation = match switched_validation {
            Ok(()) => Self::validate_switched_database(paths.database_file()).await,
            Err(error) => Err(error),
        };

        if let Err(validation_error) = switched_validation {
            let rollback = rollback_database_files(
                paths.database_file(),
                &files.backup,
                &files.failed_candidate,
            );

            return match rollback {
                Ok(()) => Err(DatabaseError::MigrationSwitch(format!(
                    "the switched database failed final validation ({validation_error}); the \
                     original database was restored"
                ))),
                Err(rollback_error) => Err(DatabaseError::MigrationSwitch(format!(
                    "the switched database failed final validation ({validation_error}) and \
                     automatic rollback failed ({rollback_error}). The original database is \
                     retained at {}",
                    files.backup.display()
                ))),
            };
        }

        Ok(())
    }

    async fn verify_or_initialize_application_id(
        pool: &SqlitePool,
        allow_new_database_claim: bool,
    ) -> DatabaseResult<()> {
        let mut connection = pool.acquire().await?;
        Self::verify_application_id_connection(&mut connection, allow_new_database_claim).await
    }

    async fn verify_application_id_connection(
        connection: &mut SqliteConnection,
        allow_new_database_claim: bool,
    ) -> DatabaseResult<()> {
        let found: i64 = sqlx::query_scalar("PRAGMA application_id")
            .fetch_one(&mut *connection)
            .await?;

        match found {
            APPLICATION_ID => Ok(()),
            0 if allow_new_database_claim => {
                // PRAGMA assignment does not accept a bound parameter.
                sqlx::query("PRAGMA application_id = 1515473749")
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            }
            0 => Err(DatabaseError::UnrecognizedDatabase),
            found => Err(DatabaseError::UnexpectedApplicationId {
                expected: APPLICATION_ID,
                found,
            }),
        }
    }

    async fn schema_is_current_pool(pool: &SqlitePool) -> DatabaseResult<bool> {
        let mut connection = pool.acquire().await?;
        Self::schema_is_current_connection(&mut connection).await
    }

    async fn schema_is_current_connection(
        connection: &mut SqliteConnection,
    ) -> DatabaseResult<bool> {
        let migration_table_exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(\
                 SELECT 1 FROM sqlite_master \
                 WHERE type = 'table' AND name = '_sqlx_migrations'\
             )",
        )
        .fetch_one(&mut *connection)
        .await?;

        if migration_table_exists != 1 {
            return Ok(false);
        }

        let applied = sqlx::query(
            "SELECT version, checksum, success \
             FROM _sqlx_migrations \
             ORDER BY version",
        )
        .fetch_all(&mut *connection)
        .await?;
        let expected: Vec<_> = MIGRATOR
            .iter()
            .filter(|migration| migration.migration_type.is_up_migration())
            .collect();

        if applied.len() != expected.len() {
            return Ok(false);
        }

        for (row, migration) in applied.iter().zip(expected) {
            let version: i64 = row.try_get("version")?;
            let checksum: Vec<u8> = row.try_get("checksum")?;
            let success: bool = row.try_get("success")?;

            if !success || version != migration.version || checksum != migration.checksum.as_ref() {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn create_consistent_snapshot(
        source: &mut SqliteConnection,
        candidate: &Path,
    ) -> DatabaseResult<()> {
        sqlx::query("VACUUM main INTO ?")
            .bind(candidate.to_string_lossy().as_ref())
            .execute(&mut *source)
            .await?;
        sync_file(candidate)
    }

    async fn migrate_and_verify_candidate(
        candidate: &Path,
        app_version: &str,
    ) -> DatabaseResult<()> {
        let mut connection = connect_connection(candidate, false).await?;
        Self::verify_application_id_connection(&mut connection, false).await?;
        Self::force_delete_journal(&mut connection).await?;
        Self::prepare_legacy_builtin_question_type_conflicts(&mut connection).await?;
        MIGRATOR.run_direct(&mut connection).await?;
        Self::ensure_database_identity_connection(&mut connection, app_version).await?;
        Self::verify_integrity_connection(&mut connection).await?;

        if !Self::schema_is_current_connection(&mut connection).await? {
            return Err(DatabaseError::IntegrityCheck(
                "the migration candidate did not reach the expected schema".to_owned(),
            ));
        }

        // Exercise WAL mode on the candidate before switching, then leave it as
        // a single self-contained file for ReplaceFileW/rename. Close and
        // reopen after enabling WAL so all statements from migration and
        // validation are finalized before the checkpoint. SQLite otherwise
        // reports SQLITE_LOCKED when a checkpoint follows on the same Windows
        // connection.
        Self::enable_wal_connection(&mut connection).await?;
        connection.close().await?;

        let mut connection = connect_connection(candidate, false).await?;
        Self::prepare_source_for_switch(&mut connection).await?;
        connection.close().await?;
        sync_file(candidate)
    }

    /// Migration 0015 was shipped before its custom-name conflict handling was
    /// hardened, so its bytes can no longer be changed. Prepare only pre-0015
    /// snapshots here, before SQLx runs that immutable migration. This work is
    /// performed on the disposable migration candidate, never on the live file.
    async fn prepare_legacy_builtin_question_type_conflicts(
        connection: &mut SqliteConnection,
    ) -> DatabaseResult<()> {
        let migration_15_applied: i64 = sqlx::query_scalar(
            "SELECT EXISTS(\
                 SELECT 1 FROM _sqlx_migrations \
                 WHERE version = 15 AND success = 1\
             )",
        )
        .fetch_one(&mut *connection)
        .await?;
        if migration_15_applied == 1 {
            return Ok(());
        }

        let required_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type = 'table' \
               AND name IN ('question_types', 'question_type_aliases')",
        )
        .fetch_one(&mut *connection)
        .await?;
        if required_tables != 2 {
            return Ok(());
        }

        let mut transaction = connection.begin().await?;
        let conflicting_types: Vec<(String, String)> = sqlx::query_as(
            "SELECT code, name FROM question_types \
             WHERE is_builtin = 0 \
               AND name_key IN ('判断题', '判断', '正误题', '是非题') \
             ORDER BY code",
        )
        .fetch_all(&mut *transaction)
        .await?;
        for (code, original_name) in conflicting_types {
            let replacement =
                unique_legacy_question_type_name(&mut transaction, &code, &original_name).await?;
            sqlx::query("UPDATE question_types SET name = ?, name_key = lower(?) WHERE code = ?")
                .bind(&replacement)
                .bind(&replacement)
                .bind(&code)
                .execute(&mut *transaction)
                .await?;
        }

        let conflicting_aliases: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, alias FROM question_type_aliases \
             WHERE alias_key IN ('判断题', '判断', '正误题', '是非题') \
             ORDER BY id",
        )
        .fetch_all(&mut *transaction)
        .await?;
        for (id, original_alias) in conflicting_aliases {
            let replacement =
                unique_legacy_question_type_alias(&mut transaction, &id, &original_alias).await?;
            sqlx::query(
                "UPDATE question_type_aliases SET alias = ?, alias_key = lower(?) WHERE id = ?",
            )
            .bind(&replacement)
            .bind(&replacement)
            .bind(&id)
            .execute(&mut *transaction)
            .await?;
        }

        transaction.commit().await?;
        Ok(())
    }

    async fn validate_switched_database(database_file: &Path) -> DatabaseResult<()> {
        let mut connection = connect_connection(database_file, false).await?;
        Self::verify_application_id_connection(&mut connection, false).await?;
        Self::verify_integrity_connection(&mut connection).await?;

        if !Self::schema_is_current_connection(&mut connection).await? {
            return Err(DatabaseError::IntegrityCheck(
                "the switched database schema does not match the application".to_owned(),
            ));
        }

        connection.close().await?;
        Ok(())
    }

    async fn hold_exclusive_lock(connection: &mut SqliteConnection) -> DatabaseResult<()> {
        let mode: String = sqlx::query_scalar("PRAGMA locking_mode = EXCLUSIVE")
            .fetch_one(&mut *connection)
            .await?;
        if !mode.eq_ignore_ascii_case("exclusive") {
            return Err(DatabaseError::MigrationSwitch(format!(
                "SQLite did not grant exclusive locking mode; it reported {mode}"
            )));
        }

        sqlx::query("BEGIN EXCLUSIVE")
            .execute(&mut *connection)
            .await?;
        sqlx::query("COMMIT").execute(&mut *connection).await?;
        Ok(())
    }

    async fn prepare_source_for_switch(connection: &mut SqliteConnection) -> DatabaseResult<()> {
        // Consume the PRAGMA through SQLITE_DONE. `fetch_one` stops after its
        // result row and can leave this statement active long enough for the
        // immediately following journal-mode change to fail with SQLITE_LOCKED.
        let checkpoint = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_all(&mut *connection)
            .await?;
        let busy: i64 = checkpoint
            .first()
            .ok_or_else(|| {
                DatabaseError::MigrationSwitch(
                    "SQLite did not return a WAL checkpoint result".to_owned(),
                )
            })?
            .try_get(0)?;
        if busy != 0 {
            return Err(DatabaseError::MigrationSwitch(format!(
                "SQLite could not checkpoint all WAL pages before switching (busy={busy})"
            )));
        }
        // Release the owned result rows before changing the persistent mode.
        drop(checkpoint);

        Self::force_delete_journal(connection).await
    }

    async fn force_delete_journal(connection: &mut SqliteConnection) -> DatabaseResult<()> {
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode = DELETE")
            .fetch_one(&mut *connection)
            .await?;
        if !journal_mode.eq_ignore_ascii_case("delete") {
            return Err(DatabaseError::MigrationSwitch(format!(
                "SQLite could not make the database self-contained; journal mode is {journal_mode}"
            )));
        }

        Ok(())
    }

    async fn enable_wal(pool: &SqlitePool) -> DatabaseResult<()> {
        let mut connection = pool.acquire().await?;
        Self::enable_wal_connection(&mut connection).await
    }

    async fn enable_wal_connection(connection: &mut SqliteConnection) -> DatabaseResult<()> {
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode = WAL")
            .fetch_one(&mut *connection)
            .await?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(DatabaseError::IntegrityCheck(format!(
                "WAL mode could not be enabled; SQLite reported {journal_mode}"
            )));
        }

        Ok(())
    }

    async fn ensure_database_identity(pool: &SqlitePool, app_version: &str) -> DatabaseResult<()> {
        let mut connection = pool.acquire().await?;
        Self::ensure_database_identity_connection(&mut connection, app_version).await
    }

    async fn ensure_database_identity_connection(
        connection: &mut SqliteConnection,
        app_version: &str,
    ) -> DatabaseResult<()> {
        let database_uuid = uuid::Uuid::now_v7().to_string();
        let now = unix_millis()?;

        sqlx::query(
            "INSERT INTO app_meta (\
                 singleton_id, database_uuid, created_by_app_version, created_at_ms\
             ) VALUES (1, ?, ?, ?) \
             ON CONFLICT(singleton_id) DO NOTHING",
        )
        .bind(database_uuid)
        .bind(app_version)
        .bind(now)
        .execute(&mut *connection)
        .await?;

        Ok(())
    }

    async fn verify_integrity_pool(pool: &SqlitePool) -> DatabaseResult<()> {
        let mut connection = pool.acquire().await?;
        Self::verify_integrity_connection(&mut connection).await
    }

    async fn verify_integrity_connection(connection: &mut SqliteConnection) -> DatabaseResult<()> {
        let result: String = sqlx::query_scalar("PRAGMA quick_check")
            .fetch_one(&mut *connection)
            .await?;
        if result != "ok" {
            return Err(DatabaseError::IntegrityCheck(result));
        }

        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut *connection)
            .await?;
        if enabled != 1 {
            return Err(DatabaseError::IntegrityCheck(
                "foreign-key enforcement is disabled".to_owned(),
            ));
        }

        if let Some(violation) = sqlx::query("PRAGMA foreign_key_check")
            .fetch_optional(&mut *connection)
            .await?
        {
            let table: String = violation.try_get("table")?;
            let parent: String = violation.try_get("parent")?;
            return Err(DatabaseError::IntegrityCheck(format!(
                "foreign-key violation in {table}, expected parent {parent}"
            )));
        }

        Ok(())
    }

    pub async fn verify_integrity(&self) -> DatabaseResult<()> {
        Self::verify_integrity_pool(&self.pool).await
    }

    pub async fn schema_version(&self) -> DatabaseResult<i64> {
        let version = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(version)
    }

    pub async fn database_uuid(&self) -> DatabaseResult<String> {
        let id = sqlx::query_scalar::<_, String>(
            "SELECT database_uuid FROM app_meta WHERE singleton_id = 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn paths(&self) -> &DatabasePaths {
        &self.paths
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

async fn unique_legacy_question_type_name(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    code: &str,
    original_name: &str,
) -> DatabaseResult<String> {
    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM question_types")
        .fetch_one(&mut **transaction)
        .await?;

    for suffix in 0..=row_count {
        let candidate = legacy_compatibility_label(original_name, code, suffix);
        let occupied: i64 = sqlx::query_scalar(
            "SELECT EXISTS(\
                 SELECT 1 FROM question_types \
                 WHERE code <> ? AND name_key = lower(?)\
             )",
        )
        .bind(code)
        .bind(&candidate)
        .fetch_one(&mut **transaction)
        .await?;
        if occupied == 0 {
            return Ok(candidate);
        }
    }

    Err(DatabaseError::MigrationSwitch(format!(
        "could not reserve a conflict-free legacy question type name for {code}"
    )))
}

async fn unique_legacy_question_type_alias(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    original_alias: &str,
) -> DatabaseResult<String> {
    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM question_type_aliases")
        .fetch_one(&mut **transaction)
        .await?;
    let compact_id = id.replace('-', "");

    for suffix in 0..=row_count {
        let candidate = legacy_compatibility_label(original_alias, &compact_id, suffix);
        let occupied: i64 = sqlx::query_scalar(
            "SELECT EXISTS(\
                 SELECT 1 FROM question_type_aliases \
                 WHERE id <> ? AND alias_key = lower(?)\
             )",
        )
        .bind(id)
        .bind(&candidate)
        .fetch_one(&mut **transaction)
        .await?;
        if occupied == 0 {
            return Ok(candidate);
        }
    }

    Err(DatabaseError::MigrationSwitch(format!(
        "could not reserve a conflict-free legacy question type alias for {id}"
    )))
}

fn legacy_compatibility_label(original: &str, discriminator: &str, suffix: i64) -> String {
    const MAX_LABEL_CHARS: usize = 40;

    let suffix = if suffix == 0 {
        String::new()
    } else {
        format!("-{suffix}")
    };
    let base = format!("原{original}-{discriminator}");
    let base_limit = MAX_LABEL_CHARS.saturating_sub(suffix.chars().count());
    let mut candidate: String = base.chars().take(base_limit).collect();
    candidate.push_str(&suffix);
    candidate
}

async fn connect_pool(
    database_file: &Path,
    create_if_missing: bool,
    max_connections: u32,
) -> DatabaseResult<SqlitePool> {
    let options = connect_options(database_file, create_if_missing);
    Ok(SqlitePoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await?)
}

async fn connect_connection(
    database_file: &Path,
    create_if_missing: bool,
) -> DatabaseResult<SqliteConnection> {
    Ok(SqliteConnection::connect_with(&connect_options(database_file, create_if_missing)).await?)
}

fn connect_options(database_file: &Path, create_if_missing: bool) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(database_file)
        .create_if_missing(create_if_missing)
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5))
}

fn file_is_nonempty(path: &Path) -> DatabaseResult<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() > 0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn sync_file(path: &Path) -> DatabaseResult<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()?;
    Ok(())
}

fn remove_owned_work_file(path: &Path) -> DatabaseResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn recover_failed_switch(
    original: &Path,
    backup: &Path,
    failed_candidate: &Path,
) -> std::io::Result<()> {
    if backup.exists() {
        if original.exists() {
            // ReplaceFileW can report a failure after doing part of its work.
            // If a backup was created, positively restore it instead of
            // guessing which database currently owns the live filename.
            rollback_database_files(original, backup, failed_candidate)?;
        } else {
            fs::rename(backup, original)?;
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(original)?
            .sync_all()?;
        return Ok(());
    }

    if original.exists() {
        return Ok(());
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "neither the active database nor its pre-migration backup exists",
    ))
}

fn rollback_database_files(
    original: &Path,
    backup: &Path,
    failed_candidate: &Path,
) -> std::io::Result<()> {
    if !backup.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("the pre-migration backup is missing: {}", backup.display()),
        ));
    }

    switch_database_files(original, backup, failed_candidate)
}

#[cfg(windows)]
fn switch_database_files(original: &Path, candidate: &Path, backup: &Path) -> std::io::Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let original = wide(original);
    let candidate = wide(candidate);
    let backup = wide(backup);
    // ReplaceFileW performs the destination replacement and creation of the
    // backup as one filesystem operation. All paths live below one data root.
    let result = unsafe {
        ReplaceFileW(
            original.as_ptr(),
            candidate.as_ptr(),
            backup.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn switch_database_files(original: &Path, candidate: &Path, backup: &Path) -> std::io::Result<()> {
    fs::rename(original, backup)?;
    if let Err(error) = fs::rename(candidate, original) {
        let rollback = fs::rename(backup, original);
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(std::io::Error::other(format!(
                "replacement failed ({error}); rollback failed ({rollback_error})"
            ))),
        };
    }

    Ok(())
}

fn unix_millis() -> DatabaseResult<i64> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(elapsed.as_millis().min(i64::MAX as u128) as i64)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sqlx::{Connection, migrate::Migrate};

    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zhitiku-db-{name}-{}",
            uuid::Uuid::now_v7().simple()
        ))
    }

    #[test]
    fn published_v15_checksum_stays_immutable() {
        const PUBLISHED_V15_CHECKSUM: &[u8] = &[
            0x66, 0x50, 0x3a, 0xb6, 0xcd, 0x61, 0x71, 0xe8, 0xe1, 0xb7, 0xa5, 0x47, 0x03, 0x41,
            0x8b, 0x5f, 0xba, 0x18, 0xba, 0xe5, 0x89, 0xac, 0xc2, 0xe0, 0x23, 0x91, 0xda, 0x89,
            0xe2, 0x67, 0x19, 0x5b, 0x57, 0x96, 0x15, 0xd9, 0x5e, 0x9b, 0x01, 0xbc, 0x24, 0xe3,
            0xb8, 0x79, 0xef, 0xad, 0x04, 0xd1,
        ];
        let migration = MIGRATOR
            .iter()
            .find(|migration| migration.version == 15)
            .expect("migration 15 must remain present");

        assert_eq!(migration.checksum.as_ref(), PUBLISHED_V15_CHECKSUM);
    }

    #[tokio::test]
    async fn fresh_database_initializes_and_reopens_without_backup() {
        let root = test_root("fresh");
        let database = Database::open(&root, "test").await.unwrap();
        let database_uuid = database.database_uuid().await.unwrap();
        assert_eq!(
            database.schema_version().await.unwrap(),
            Database::current_schema_version()
        );
        database.close().await;

        let reopened = Database::open(&root, "test").await.unwrap();
        assert_eq!(reopened.database_uuid().await.unwrap(), database_uuid);
        reopened.close().await;

        assert_eq!(fs::read_dir(root.join("backup")).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn published_v15_database_upgrades_to_v16() {
        let root = test_root("published-v15-upgrade");
        let paths = DatabasePaths::new(&root);
        paths.prepare().unwrap();
        let mut connection = connect_connection(paths.database_file(), true)
            .await
            .unwrap();
        sqlx::query("PRAGMA application_id = 1515473749")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.ensure_migrations_table().await.unwrap();
        for migration in MIGRATOR.iter().filter(|migration| migration.version <= 15) {
            connection.apply(migration).await.unwrap();
        }

        let database_uuid = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO app_meta (singleton_id, database_uuid, created_by_app_version, \
             created_at_ms) VALUES (1, ?, '0.1.52', 1000)",
        )
        .bind(&database_uuid)
        .execute(&mut connection)
        .await
        .unwrap();
        let applied_v15_checksum: Vec<u8> = sqlx::query_scalar(
            "SELECT checksum FROM _sqlx_migrations WHERE version = 15 AND success = 1",
        )
        .fetch_one(&mut connection)
        .await
        .unwrap();
        let expected_v15_checksum = MIGRATOR
            .iter()
            .find(|migration| migration.version == 15)
            .unwrap()
            .checksum
            .as_ref();
        assert_eq!(applied_v15_checksum, expected_v15_checksum);
        connection.close().await.unwrap();

        let database = Database::open(&root, "0.1.52-test").await.unwrap();
        let latest_schema_version = MIGRATOR
            .iter()
            .map(|migration| migration.version)
            .max()
            .unwrap();
        assert_eq!(
            database.schema_version().await.unwrap(),
            latest_schema_version
        );
        assert_eq!(database.database_uuid().await.unwrap(), database_uuid);
        database.close().await;

        let backups: Vec<_> = fs::read_dir(paths.backup_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("zhitiku-pre-migration-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        drop(backups);
        for attempt in 0..10 {
            match fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(_) if attempt < 9 => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean v15 upgrade test directory: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn v20_foreign_key_apply_failure_upgrades_to_deferred_retry() {
        let root = test_root("v20-deferred-cloud-apply");
        let paths = DatabasePaths::new(&root);
        paths.prepare().unwrap();
        let mut connection = connect_connection(paths.database_file(), true)
            .await
            .unwrap();
        sqlx::query("PRAGMA application_id = 1515473749")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.ensure_migrations_table().await.unwrap();
        for migration in MIGRATOR.iter().filter(|migration| migration.version <= 20) {
            connection.apply(migration).await.unwrap();
        }

        sqlx::query(
            "INSERT INTO app_meta (singleton_id, database_uuid, created_by_app_version, created_at_ms) VALUES (1, ?, '0.1.75', 1)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut connection)
        .await
        .unwrap();
        let account_id = uuid::Uuid::now_v7().to_string();
        let deferred_id = uuid::Uuid::now_v7().to_string();
        let content_id = uuid::Uuid::now_v7().to_string();
        for (id, message) in [
            (
                &deferred_id,
                "云端内容无法安全写入本地：本地数据库操作失败：error returned from database: (code: 787) FOREIGN KEY constraint failed",
            ),
            (&content_id, "本机和云端都修改了这一项。"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO cloud_sync_conflicts (
                    id, account_id, entity_kind, entity_id, local_payload_json,
                    remote_payload_json, detected_at_ms, message
                ) VALUES (?, ?, 'question', ?, NULL, '{}', 1, ?)
                "#,
            )
            .bind(id)
            .bind(&account_id)
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(message)
            .execute(&mut connection)
            .await
            .unwrap();
        }
        connection.close().await.unwrap();

        let database = Database::open(&root, "0.1.76-test").await.unwrap();
        let deferred_kind = sqlx::query_scalar::<_, String>(
            "SELECT conflict_kind FROM cloud_sync_conflicts WHERE id = ?",
        )
        .bind(&deferred_id)
        .fetch_one(database.pool())
        .await
        .unwrap();
        let content_kind = sqlx::query_scalar::<_, String>(
            "SELECT conflict_kind FROM cloud_sync_conflicts WHERE id = ?",
        )
        .bind(&content_id)
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(deferred_kind, "deferred_apply");
        assert_eq!(content_kind, "content");
        database.close().await;

        for attempt in 0..10 {
            match fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(_) if attempt < 9 => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean v20 upgrade test directory: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn nonempty_database_with_zero_application_id_is_rejected() {
        let root = test_root("unrecognized");
        let paths = DatabasePaths::new(&root);
        paths.prepare().unwrap();
        let mut connection = connect_connection(paths.database_file(), true)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE app_meta (value TEXT)")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();

        let error = Database::open(&root, "test").await.unwrap_err();
        assert!(matches!(error, DatabaseError::UnrecognizedDatabase));
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn older_database_is_migrated_via_copy_and_original_is_retained() {
        let root = test_root("upgrade");
        let paths = DatabasePaths::new(&root);
        paths.prepare().unwrap();
        let mut connection = connect_connection(paths.database_file(), true)
            .await
            .unwrap();
        sqlx::query("PRAGMA application_id = 1515473749")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.ensure_migrations_table().await.unwrap();
        connection
            .apply(MIGRATOR.iter().next().unwrap())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO app_meta (\
                 singleton_id, database_uuid, created_by_app_version, created_at_ms\
             ) VALUES (1, ?, 'old-test', 1)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut connection)
        .await
        .unwrap();
        connection.close().await.unwrap();

        let database = Database::open(&root, "test").await.unwrap();
        assert_eq!(
            database.schema_version().await.unwrap(),
            Database::current_schema_version()
        );
        database.close().await;

        let backups: Vec<_> = fs::read_dir(paths.backup_dir())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(
            backups[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("zhitiku-pre-migration-")
        );

        let mut backup = connect_connection(&backups[0], false).await.unwrap();
        let version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success = 1",
        )
        .fetch_one(&mut backup)
        .await
        .unwrap();
        let application_id: i64 = sqlx::query_scalar("PRAGMA application_id")
            .fetch_one(&mut backup)
            .await
            .unwrap();
        assert_eq!(version, 1);
        assert_eq!(application_id, APPLICATION_ID);
        backup.close().await.unwrap();

        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn populated_v12_database_upgrades_without_losing_user_content() {
        let root = test_root("populated-v12-upgrade");
        let paths = DatabasePaths::new(&root);
        paths.prepare().unwrap();
        let mut connection = connect_connection(paths.database_file(), true)
            .await
            .unwrap();
        sqlx::query("PRAGMA application_id = 1515473749")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.ensure_migrations_table().await.unwrap();
        for migration in MIGRATOR.iter().filter(|migration| migration.version <= 12) {
            connection.apply(migration).await.unwrap();
        }

        let database_uuid = uuid::Uuid::now_v7().to_string();
        let subject_id = uuid::Uuid::now_v7().to_string();
        let chapter_id = uuid::Uuid::now_v7().to_string();
        let tag_id = uuid::Uuid::now_v7().to_string();
        let question_id = uuid::Uuid::now_v7().to_string();
        let option_id = uuid::Uuid::now_v7().to_string();
        let paper_id = uuid::Uuid::now_v7().to_string();
        let paper_item_id = uuid::Uuid::now_v7().to_string();

        sqlx::query(
            "INSERT INTO app_meta (singleton_id, database_uuid, created_by_app_version, \
             created_at_ms) VALUES (1, ?, '0.1.44', 1000)",
        )
        .bind(&database_uuid)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO subjects (id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, '旧版数学', '旧版数学', 1024, 1000, 1000)",
        )
        .bind(&subject_id)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chapters (id, subject_id, name, name_key, sort_order, created_at_ms, \
             updated_at_ms) VALUES (?, ?, '第一章', '第一章', 1024, 1000, 1000)",
        )
        .bind(&chapter_id)
        .bind(&subject_id)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO tags (id, name, name_key, created_at_ms, updated_at_ms) \
             VALUES (?, '旧库标签', '旧库标签', 1000, 1000)",
        )
        .bind(&tag_id)
        .execute(&mut connection)
        .await
        .unwrap();

        let stem = serde_json::json!({
            "schemaVersion": 1,
            "html": "<p>旧版题目：1 + 1 等于多少？</p>",
            "plainText": "旧版题目：1 + 1 等于多少？"
        })
        .to_string();
        let answer = serde_json::json!({
            "schemaVersion": 1,
            "html": "<p>A</p>",
            "plainText": "A"
        })
        .to_string();
        let empty = serde_json::json!({
            "schemaVersion": 1,
            "html": "",
            "plainText": ""
        })
        .to_string();
        sqlx::query(
            "INSERT INTO questions (id, question_type, subject_id, chapter_id, \
             content_schema_version, stem_json, answer_json, explanation_json, stem_plain, \
             options_plain, answer_plain, explanation_plain, tags_plain, fingerprint_version, \
             exact_fingerprint, content_version, created_at_ms, updated_at_ms) \
             VALUES (?, 'single_choice', ?, ?, 1, ?, ?, ?, '旧版题目：1 + 1 等于多少？', \
             'A. 2', 'A', '', '旧库标签', 1, ?, 3, 1000, 2000)",
        )
        .bind(&question_id)
        .bind(&subject_id)
        .bind(&chapter_id)
        .bind(&stem)
        .bind(&answer)
        .bind(&empty)
        .bind(vec![7u8; 32])
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO question_options (id, question_id, position, content_json, plain_text, \
             created_at_ms, updated_at_ms) VALUES (?, ?, 0, ?, '2', 1000, 1000)",
        )
        .bind(&option_id)
        .bind(&question_id)
        .bind(
            serde_json::json!({
                "schemaVersion": 1,
                "html": "<p>2</p>",
                "plainText": "2"
            })
            .to_string(),
        )
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO question_tags (question_id, tag_id, created_at_ms) VALUES (?, ?, 1000)",
        )
        .bind(&question_id)
        .bind(&tag_id)
        .execute(&mut connection)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO papers (id, title, composition_mode, paper_status, \
             subject_summary_text, export_content_mode, row_version, created_at_ms, \
             updated_at_ms, saved_at_ms, last_saved_at_ms) \
             VALUES (?, '旧版期末试卷', 'manual', 'saved', '旧版数学', 'paper_only', 2, \
             1000, 2000, 2000, 2000)",
        )
        .bind(&paper_id)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO paper_items (id, paper_id, source_question_id, position, question_type, \
             subject_id_snapshot, chapter_id_snapshot, tag_ids_snapshot_json, \
             snapshot_schema_version, snapshot_json, source_content_version, \
             source_exact_fingerprint, created_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, 0, 'single_choice', ?, ?, ?, 1, ?, 3, ?, 1000, 1000)",
        )
        .bind(&paper_item_id)
        .bind(&paper_id)
        .bind(&question_id)
        .bind(&subject_id)
        .bind(&chapter_id)
        .bind(serde_json::json!([tag_id.clone()]).to_string())
        .bind(serde_json::json!({ "questionId": question_id.clone(), "legacy": true }).to_string())
        .bind(vec![7u8; 32])
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO backup_records (id, backup_kind, status, archive_rel_path, \
             display_filename, format_version, database_schema_version, source_database_uuid, \
             source_app_version, contents_summary_json, created_at_ms, completed_at_ms, \
             last_verified_at_ms) VALUES (?, 'manual', 'ready', 'legacy-manual.tqb', \
             'legacy-manual.tqb', 1, 12, ?, '0.1.44', '{}', 1000, 1000, 1000)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&database_uuid)
        .execute(&mut connection)
        .await
        .unwrap();
        connection.close().await.unwrap();

        let database = Database::open(&root, "0.1.45").await.unwrap();
        assert_eq!(
            database.schema_version().await.unwrap(),
            Database::current_schema_version()
        );
        assert_eq!(database.database_uuid().await.unwrap(), database_uuid);
        let counts: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM questions), \
             (SELECT COUNT(*) FROM question_options), (SELECT COUNT(*) FROM question_tags), \
             (SELECT COUNT(*) FROM papers), (SELECT COUNT(*) FROM paper_items), \
             (SELECT COUNT(*) FROM backup_records WHERE backup_kind = 'manual')",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(counts, (1, 1, 1, 1, 1, 1));
        let restored_stem: String =
            sqlx::query_scalar("SELECT stem_plain FROM questions WHERE id = ?")
                .bind(&question_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(restored_stem, "旧版题目：1 + 1 等于多少？");
        let fts_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM question_fts")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(fts_rows, 1);
        let automatic_settings: (i64, i64, i64) = sqlx::query_as(
            "SELECT automatic_backup_enabled, automatic_backup_interval_days, \
             automatic_backup_retention_count FROM app_settings WHERE singleton_id = 1",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(automatic_settings, (1, 7, 5));
        let foreign_key_errors: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(foreign_key_errors, 0);
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(integrity, "ok");
        database.close().await;

        let pre_migration_backups: Vec<_> = fs::read_dir(paths.backup_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("zhitiku-pre-migration-")
            })
            .collect();
        assert_eq!(pre_migration_backups.len(), 1);
        for attempt in 0..10 {
            match fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(_) if attempt < 9 => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("failed to clean upgrade test directory: {error}"),
            }
        }
    }

    #[tokio::test]
    async fn v14_question_types_upgrade_merges_retired_types_without_losing_content() {
        let root = test_root("v14-question-types-upgrade");
        let paths = DatabasePaths::new(&root);
        paths.prepare().unwrap();
        let mut connection = connect_connection(paths.database_file(), true)
            .await
            .unwrap();
        sqlx::query("PRAGMA application_id = 1515473749")
            .execute(&mut connection)
            .await
            .unwrap();
        connection.ensure_migrations_table().await.unwrap();
        for migration in MIGRATOR.iter().filter(|migration| migration.version <= 14) {
            connection.apply(migration).await.unwrap();
        }

        let database_uuid = uuid::Uuid::now_v7().to_string();
        let subject_id = uuid::Uuid::now_v7().to_string();
        let chapter_id = uuid::Uuid::now_v7().to_string();
        let application_id = uuid::Uuid::now_v7().to_string();
        let case_id = uuid::Uuid::now_v7().to_string();
        let paper_id = uuid::Uuid::now_v7().to_string();
        let question_draft_id = uuid::Uuid::now_v7().to_string();
        let word_draft_id = uuid::Uuid::now_v7().to_string();
        let word_session_id = uuid::Uuid::now_v7().to_string();
        let word_item_id = uuid::Uuid::now_v7().to_string();
        let document_draft_id = uuid::Uuid::now_v7().to_string();
        let custom_judgment_code = "custom_11111111111111111111111111111111";
        let occupied_old_suffix_code = "custom_22222222222222222222222222222222";
        let custom_alias_id = "33333333-3333-7333-8333-333333333333";

        sqlx::query(
            "INSERT INTO app_meta (singleton_id, database_uuid, created_by_app_version, \
             created_at_ms) VALUES (1, ?, '0.1.51', 1000)",
        )
        .bind(&database_uuid)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO question_types (code, name, name_key, behavior, is_builtin, is_enabled, \
             sort_order, default_options_json, created_at_ms, updated_at_ms) VALUES \
             (?, '判断题', '判断题', 'open_response', 0, 1, 70, '[]', 1000, 1000), \
             (?, '判断题原自定义', '判断题原自定义', 'open_response', 0, 1, 80, '[]', 1000, 1000)",
        )
        .bind(custom_judgment_code)
        .bind(occupied_old_suffix_code)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO question_type_aliases \
             (id, question_type_code, alias, alias_key, created_at_ms) \
             VALUES (?, ?, '判断', '判断', 1000)",
        )
        .bind(custom_alias_id)
        .bind(custom_judgment_code)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO subjects (id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, '旧题库', '旧题库', 1024, 1000, 1000)",
        )
        .bind(&subject_id)
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chapters (id, subject_id, name, name_key, sort_order, created_at_ms, \
             updated_at_ms) VALUES (?, ?, '旧章节', '旧章节', 1024, 1000, 1000)",
        )
        .bind(&chapter_id)
        .bind(&subject_id)
        .execute(&mut connection)
        .await
        .unwrap();

        let empty_rich = serde_json::json!({
            "schemaVersion": 1,
            "html": "",
            "plainText": ""
        })
        .to_string();
        for (question_id, question_type, stem_plain, fingerprint_byte) in [
            (&application_id, "application", "保留综合应用题题干", 21_u8),
            (&case_id, "case_analysis", "保留案例分析题题干", 22_u8),
        ] {
            let stem = serde_json::json!({
                "schemaVersion": 1,
                "html": format!("<p>{stem_plain}</p>"),
                "plainText": stem_plain
            })
            .to_string();
            let answer = serde_json::json!({
                "schemaVersion": 1,
                "html": "<p>保留原答案</p>",
                "plainText": "保留原答案"
            })
            .to_string();
            sqlx::query(
                "INSERT INTO questions (id, question_type, subject_id, chapter_id, \
                 content_schema_version, stem_json, answer_json, explanation_json, stem_plain, \
                 options_plain, answer_plain, explanation_plain, tags_plain, fingerprint_version, \
                 exact_fingerprint, content_version, created_at_ms, updated_at_ms) \
                 VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, '', '保留原答案', '', '', 1, ?, 1, 1000, 1000)",
            )
            .bind(question_id)
            .bind(question_type)
            .bind(&subject_id)
            .bind(&chapter_id)
            .bind(stem)
            .bind(&answer)
            .bind(&empty_rich)
            .bind(stem_plain)
            .bind(vec![fingerprint_byte; 32])
            .execute(&mut connection)
            .await
            .unwrap();
        }

        let generation_config = serde_json::json!({
            "questionTypeCounts": {
                "single_choice": 1,
                "short_answer": 2,
                "application": 3,
                "case_analysis": 4
            }
        })
        .to_string();
        sqlx::query(
            "INSERT INTO papers (id, title, composition_mode, paper_status, generation_config_json, \
             subject_summary_text, export_content_mode, row_version, created_at_ms, updated_at_ms) \
             VALUES (?, '旧版自动试卷', 'automatic', 'draft', ?, '旧题库', 'paper_only', 1, 1000, 1000)",
        )
        .bind(&paper_id)
        .bind(generation_config)
        .execute(&mut connection)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO drafts (id, draft_key, draft_kind, payload_schema_version, payload_json, \
             autosaved_at_ms, created_at_ms, updated_at_ms) \
             VALUES (?, 'question:create', 'question_create', 1, ?, 1000, 1000, 1000), \
                    (?, 'word_import:active', 'word_import_preview', 1, ?, 1000, 1000, 1000)",
        )
        .bind(&question_draft_id)
        .bind(serde_json::json!({ "type": "application" }).to_string())
        .bind(&word_draft_id)
        .bind(
            serde_json::json!({
                "schemaVersion": 1,
                "items": [{ "payload": { "type": "case_analysis" } }]
            })
            .to_string(),
        )
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO word_import_sessions \
             (id, draft_id, source_filename, source_sha256, source_byte_size, parser_version, \
              status, diagnostics_json, created_at_ms, updated_at_ms) \
             VALUES (?, ?, 'legacy.docx', ?, 1, 'legacy', 'reviewing', '[]', 1000, 1000)",
        )
        .bind(&word_session_id)
        .bind(&word_draft_id)
        .bind(vec![7_u8; 32])
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO word_import_items \
             (id, session_id, source_ordinal, selected, item_schema_version, payload_json, \
              validation_json, duplicate_kind, item_status, created_at_ms, updated_at_ms) \
             VALUES (?, ?, 0, 1, 1, ?, '[]', 'none', 'review', 1000, 1000)",
        )
        .bind(&word_item_id)
        .bind(&word_session_id)
        .bind(serde_json::json!({ "type": "application" }).to_string())
        .execute(&mut connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO document_question_drafts \
             (id, draft_key, payload_schema_version, payload_json, autosaved_at_ms, \
              created_at_ms, updated_at_ms) \
             VALUES (?, 'question_document:active', 1, ?, 1000, 1000, 1000)",
        )
        .bind(&document_draft_id)
        .bind(
            serde_json::json!({
                "schemaVersion": 1,
                "items": [{ "payload": { "type": "case_analysis" } }]
            })
            .to_string(),
        )
        .execute(&mut connection)
        .await
        .unwrap();

        for (position, question_id, question_type, stem_plain, fingerprint_byte) in [
            (
                0_i64,
                &application_id,
                "application",
                "保留综合应用题题干",
                21_u8,
            ),
            (
                1_i64,
                &case_id,
                "case_analysis",
                "保留案例分析题题干",
                22_u8,
            ),
        ] {
            sqlx::query(
                "INSERT INTO paper_items (id, paper_id, source_question_id, position, question_type, \
                 subject_id_snapshot, chapter_id_snapshot, tag_ids_snapshot_json, \
                 snapshot_schema_version, snapshot_json, source_content_version, \
                 source_exact_fingerprint, created_at_ms, updated_at_ms) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, '[]', 1, ?, 1, ?, 1000, 1000)",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&paper_id)
            .bind(question_id)
            .bind(position)
            .bind(question_type)
            .bind(&subject_id)
            .bind(&chapter_id)
            .bind(
                serde_json::json!({
                    "type": question_type,
                    "stem": { "plainText": stem_plain }
                })
                .to_string(),
            )
            .bind(vec![fingerprint_byte; 32])
            .execute(&mut connection)
            .await
            .unwrap();
        }
        connection.close().await.unwrap();

        let database = Database::open(&root, "0.1.52-test").await.unwrap();
        let builtins: Vec<(String, String)> = sqlx::query_as(
            "SELECT code, default_options_json FROM question_types WHERE is_builtin = 1 \
             ORDER BY sort_order",
        )
        .fetch_all(database.pool())
        .await
        .unwrap();
        assert_eq!(
            builtins,
            vec![
                ("single_choice".to_owned(), "[]".to_owned()),
                ("multiple_choice".to_owned(), "[]".to_owned()),
                ("fill_blank".to_owned(), "[]".to_owned()),
                ("true_false".to_owned(), r#"["正确","错误"]"#.to_owned()),
                ("short_answer".to_owned(), "[]".to_owned()),
            ]
        );
        let renamed_custom_type: (String, String) =
            sqlx::query_as("SELECT name, name_key FROM question_types WHERE code = ?")
                .bind(custom_judgment_code)
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert!(renamed_custom_type.0.starts_with("原判断题-"));
        assert_eq!(renamed_custom_type.0, renamed_custom_type.1);
        let occupied_old_suffix_name: String =
            sqlx::query_scalar("SELECT name FROM question_types WHERE code = ?")
                .bind(occupied_old_suffix_code)
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(occupied_old_suffix_name, "判断题原自定义");
        let preserved_alias: (String, String) =
            sqlx::query_as("SELECT alias, alias_key FROM question_type_aliases WHERE id = ?")
                .bind(custom_alias_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert!(preserved_alias.0.starts_with("原判断-"));
        assert_eq!(preserved_alias.0, preserved_alias.1);
        let migrated_questions: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, question_type, stem_plain FROM questions ORDER BY stem_plain",
        )
        .fetch_all(database.pool())
        .await
        .unwrap();
        assert_eq!(migrated_questions.len(), 2);
        assert!(migrated_questions.iter().all(|row| row.1 == "short_answer"));
        assert!(
            migrated_questions
                .iter()
                .any(|row| row.2 == "保留综合应用题题干")
        );
        assert!(
            migrated_questions
                .iter()
                .any(|row| row.2 == "保留案例分析题题干")
        );

        let migrated_items: Vec<(String, String)> = sqlx::query_as(
            "SELECT question_type, snapshot_json FROM paper_items ORDER BY position",
        )
        .fetch_all(database.pool())
        .await
        .unwrap();
        assert_eq!(migrated_items.len(), 2);
        assert!(migrated_items.iter().all(|row| row.0 == "short_answer"));
        assert!(migrated_items.iter().all(|row| {
            serde_json::from_str::<serde_json::Value>(&row.1).unwrap()["type"] == "short_answer"
        }));

        let migrated_config: String =
            sqlx::query_scalar("SELECT generation_config_json FROM papers WHERE id = ?")
                .bind(&paper_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        let migrated_config: serde_json::Value = serde_json::from_str(&migrated_config).unwrap();
        assert_eq!(migrated_config["questionTypeCounts"]["short_answer"], 9);
        assert!(
            migrated_config["questionTypeCounts"]
                .get("application")
                .is_none()
        );

        for payload in sqlx::query_scalar::<_, String>(
            "SELECT payload_json FROM drafts WHERE id IN (?, ?) \
             UNION ALL SELECT payload_json FROM word_import_items WHERE id = ? \
             UNION ALL SELECT payload_json FROM document_question_drafts WHERE id = ?",
        )
        .bind(&question_draft_id)
        .bind(&word_draft_id)
        .bind(&word_item_id)
        .bind(&document_draft_id)
        .fetch_all(database.pool())
        .await
        .unwrap()
        {
            assert!(payload.contains("short_answer"));
            assert!(!payload.contains("application"));
            assert!(!payload.contains("case_analysis"));
        }
        assert!(
            migrated_config["questionTypeCounts"]
                .get("case_analysis")
                .is_none()
        );

        let foreign_key_errors: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(foreign_key_errors, 0);
        database.close().await;
        fs::remove_dir_all(root).unwrap();
    }
}
