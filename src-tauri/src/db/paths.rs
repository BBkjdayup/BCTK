use std::{
    fs,
    path::{Path, PathBuf},
};

use super::DatabaseResult;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabasePaths {
    data_root: PathBuf,
    database_dir: PathBuf,
    database_file: PathBuf,
    resources_dir: PathBuf,
    resource_staging_dir: PathBuf,
    templates_dir: PathBuf,
    backup_dir: PathBuf,
    export_dir: PathBuf,
}

impl DatabasePaths {
    pub fn new(data_root: impl Into<PathBuf>) -> Self {
        let data_root = data_root.into();
        let database_dir = data_root.join("database");
        let resources_dir = data_root.join("resources");

        Self {
            database_file: database_dir.join("zhitiku.sqlite3"),
            resource_staging_dir: resources_dir.join(".staging"),
            templates_dir: data_root.join("templates"),
            backup_dir: data_root.join("backup"),
            export_dir: data_root.join("export"),
            data_root,
            database_dir,
            resources_dir,
        }
    }

    pub fn prepare(&self) -> DatabaseResult<()> {
        for directory in [
            &self.data_root,
            &self.database_dir,
            &self.resources_dir,
            &self.resource_staging_dir,
            &self.templates_dir,
            &self.backup_dir,
            &self.export_dir,
        ] {
            fs::create_dir_all(directory)?;
        }

        Ok(())
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn database_file(&self) -> &Path {
        &self.database_file
    }

    pub fn resources_dir(&self) -> &Path {
        &self.resources_dir
    }

    pub fn resource_staging_dir(&self) -> &Path {
        &self.resource_staging_dir
    }

    pub fn templates_dir(&self) -> &Path {
        &self.templates_dir
    }

    pub fn backup_dir(&self) -> &Path {
        &self.backup_dir
    }
}
