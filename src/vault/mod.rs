use crate::cli;
use any_derive;
use diesel::prelude::*;
use fs_extra;
use serde::de;
use serde::export::Formatter;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::io::Write;
use std::{env, fmt, fs, io, path, result};
use uuid;

// TODO (rohany): Where's the right place to put this? Maybe the right thing
//  is to just have module imports in mod.rs, and everything else in separate files?
mod db;
use db::schema;
use db::schema::{experiment, experiment_instance};
use diesel;
use diesel::sql_types::{Integer, Text};

/// Result is an alias for Result<T, VaultError>. It is used for uniformity
/// among the different vault functions.
pub type Result<T> = result::Result<T, VaultError>;

/// CommandResult is the type that is returned from execution of a VaultCommand.
type CommandResult = Result<Box<dyn VaultResult>>;

/// VaultCommand is a common trait that command drivers will implement.
trait VaultCommmand {
    /// execute is the driver method for a VaultCommand. It returns an object
    /// that can be Displayed to stdout, or inspected for testing.
    fn execute(&self) -> CommandResult;
}

/// VaultResult is underlying type returned from a VaultCommmand. The idea is that
/// the resulting type is castable down to a particular implementation, and also
/// displayable via the command line.
pub trait VaultResult: util::AsAny + fmt::Display {}

#[derive(any_derive::AsAny)]
/// VaultUnit is used to return nothing from a vault command.
struct VaultUnit {}
impl VaultResult for VaultUnit {}
impl VaultUnit {
    fn new() -> CommandResult {
        Ok(Box::new(VaultUnit {}))
    }
}
impl fmt::Display for VaultUnit {
    fn fmt(&self, _: &mut Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
/// Experiment is a struct corresponding to data about a registered experiment.
struct Experiment {
    /// name is the name of the experiment.
    name: String,
    /// latest_instance_id is the instance of the most recent stored instance
    /// of the experiment, if one exists.
    latest_instance_id: Option<uuid::Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
/// ExperimentInstance is a struct that represents metadata about a particular
/// instance of an experiment.
struct ExperimentInstance {
    /// The name of the experiment that this is an instance of.
    name: String,
    /// User-defined metadata about the instance.
    meta: ExperimentMetaCollection,
    // TODO (rohany): Figure out what concrete type we'll use here.
    timestamp: String,
    /// The stable ID of this instance. It is used to construct the physical
    /// file path that the instance is stored in.
    id: uuid::Uuid,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
/// ExperimentMetaCollection represents a collection of user-defined experiment
/// instance metadata.
struct ExperimentMetaCollection {
    /// metas is a vector of ExperimentMeta's.
    metas: Vec<ExperimentMeta>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
/// ExperimentMeta is an individual piece of user-defined experiment instance metadata.
/// The metadata is a key-value pair. This is currently stored explicitly, but could be
/// dumped into an unstructured database for richer queries.
struct ExperimentMeta {
    key: String,
    value: String,
}

/// Vault is the main structure to interface with a Vault instance.
struct Vault {
    /// base_dir is the string of the directory that contains the instance.
    base_dir: String,
    /// conn is the diesel connection to the Vault instance's database.
    conn: SqliteConnection,
}

impl Vault {
    /// new creates a new Vault. It uses the environment's VAULT_DIR as the
    /// instance's base_dir.
    fn new(vault_dir: Option<String>) -> Result<Vault> {
        let dir = match vault_dir {
            Some(s) => s,
            None => (match env::var_os("VAULT_DIR") {
                Some(dir) => dir,
                None => return Err(VaultError::UnsetInstanceDirectory),
            })
            .to_str()
            .unwrap()
            .to_string(),
        };
        Vault::new_from_dir(dir.as_str())
    }

    /// new_from_dir creates a new Vault from the input instance directory.
    fn new_from_dir(instance_dir: &str) -> Result<Vault> {
        // TODO (rohany): Perform some validation on the Vault instance.
        Ok(Vault {
            base_dir: instance_dir.to_string(),
            conn: db::open(instance_dir),
        })
    }

    /// get_experiment returns the schema::Experiment with the target name, or None
    /// if the experiment doesn't yet exist in the database.
    fn get_experiment(&self, name: &str) -> Result<Option<schema::Experiment>> {
        // Query the database.
        let res = util::wrap_db_error(
            experiment::table
                .filter(experiment::name.like(name))
                .first::<schema::Experiment>(&self.conn)
                .optional(),
        )?;
        Ok(res)
    }

    /// get_experiment_error is the same as get_experiment but will raise a
    /// VaultError::UnknownExperimentError if the experiment does not exist.
    fn get_experiment_err(&self, name: &str) -> Result<schema::Experiment> {
        match self.get_experiment(name)? {
            Some(e) => Ok(e),
            None => Err(VaultError::UnknownExperimentError(name.to_string())),
        }
    }

    /// store_instance stores the target experiment instance directory into the vault
    /// under the target name. This operation performs a recursive copy from the
    /// target directory into the experiment's directory.
    fn store_instance(&mut self, target: &str, id: &str) -> Result<()> {
        // Create options for the copy.
        let mut options = fs_extra::dir::CopyOptions::new();
        // copy_inside allows us to copy the directory into a new name.
        options.copy_inside = true;
        let destination = self.make_instance_path(id);
        match fs_extra::dir::copy(target, destination, &options) {
            Ok(_) => Ok(()),
            Err(e) => Err(VaultError::IOStringError(e.to_string())),
        }
    }

    /// register_experiment registers a new experiment in the vault.
    fn register_experiment(&mut self, name: &str) -> Result<()> {
        // See whether there exists such an experiment already.
        match self.get_experiment(name)? {
            // If so, error out.
            Some(_) => {
                return Err(VaultError::DuplicateExperimentError(
                    name.to_string().clone(),
                ));
            }
            // Otherwise, insert it.
            None => {
                // Insert the experiment into the database.
                let exp = schema::ExperimentInsert { name };
                let _ = util::wrap_db_error(
                    diesel::insert_into(experiment::table)
                        .values(exp)
                        .execute(&mut self.conn),
                )?;
            }
        };
        Ok(())
    }

    /// make_instance_path returns the physical path to the directory of the
    /// instance with the input ID.
    fn make_instance_path(&self, id: &str) -> String {
        format!("{}/{}", self.base_dir, id)
    }

    #[cfg(test)]
    /// get_experiment_instances is a method for use in testing. It returns the
    /// UUID's of all of the instances that have been stored in the vault for a
    /// particular experiment.
    fn get_experiment_instances(&self, name: &str) -> Result<Vec<String>> {
        let experiment = self.get_experiment_err(name)?;
        util::wrap_db_error(
            experiment_instance::table
                .filter(experiment_instance::experiment_id.eq(experiment.id))
                .select(experiment_instance::uuid)
                .get_results::<String>(&self.conn),
        )
    }
}

#[derive(Debug)]
/// AddExperimentMeta represents a vault add-meta command.
struct AddExperimentMeta {
    /// directory is the directory corresponding to an experiment instance.
    directory: String,
    meta: AddExperimentMetaArgs,
}

#[derive(Debug)]
/// AddExperimentMetaArgs represent the kind of metadata that can be added
/// using an add-meta command.
enum AddExperimentMetaArgs {
    KV { key: String, value: String },
    JSON { json: String },
}

impl VaultCommmand for AddExperimentMeta {
    fn execute(&self) -> CommandResult {
        // Get a handle on the experiment's metadata.
        let mut handle = ExperimentMetaCollectionHandle::new(self.directory.as_str())?;
        // Construct the metadata out of the user's input.
        let metas = match &self.meta {
            AddExperimentMetaArgs::KV { key, value } => vec![ExperimentMeta {
                key: key.clone(),
                value: value.clone(),
            }],
            AddExperimentMetaArgs::JSON { json } => {
                // In the JSON case, we need to attempt to deserialize the JSON into a map.
                let parsed: serde_json::Value = match serde_json::from_str(&json) {
                    Ok(o) => o,
                    Err(e) => return Err(VaultError::JSONError(e)),
                };
                // Get out the parsed data as a Dict.
                let data = match parsed.as_object() {
                    Some(m) => m,
                    None => {
                        return Err(VaultError::InvalidJSONError(
                            "expected JSON dictionary".into(),
                        ))
                    }
                };
                // TODO (rohany): For now, I'm not allowing nested JSON objects.
                data.iter()
                    .map(|v| {
                        let value = match v.1 {
                            serde_json::Value::Bool(b) => b.to_string(),
                            serde_json::Value::Number(n) => n.to_string(),
                            serde_json::Value::String(s) => s.clone(),
                            // The remaining cases of NULL, ARRAY and OBJECT are errors.
                            _ => {
                                return Err(VaultError::InvalidJSONError(
                                    format! {"{} was not a string", v.1.to_string()},
                                ))
                            }
                        };
                        Ok(ExperimentMeta {
                            key: v.0.clone(),
                            value,
                        })
                    })
                    .collect::<Result<Vec<ExperimentMeta>>>()?
            }
        };

        // TODO (rohany): Make this loop not an O(N^2) loop.
        // Add all of the metadata into the instance meta.
        for meta in metas.iter() {
            // See if this key exists already.
            match handle
                .meta
                .metas
                .iter_mut()
                .filter(|m| m.key.as_str() == &meta.key)
                .next()
            {
                // If the key exists already, replace the value with the new one.
                Some(v) => v.value = meta.value.clone(),
                // Otherwise, add the new key.
                None => handle.meta.metas.push(ExperimentMeta {
                    key: meta.key.clone(),
                    value: meta.value.clone(),
                }),
            }
        }
        handle.flush()?;
        VaultUnit::new()
    }
}

/// ListInstanceMeta represents a vault list-meta command.
struct ListInstanceMeta {
    /// directory is the directory corresponding to an experiment instance.
    directory: String,
}

#[derive(any_derive::AsAny)]
/// ListInstanceMetaResult is the result of a list-meta command.
struct ListInstanceMetaResult {
    meta: ExperimentMetaCollection,
}
impl VaultResult for ListInstanceMetaResult {}
impl ListInstanceMetaResult {
    fn new(meta: ExperimentMetaCollection) -> CommandResult {
        return Ok(Box::new(ListInstanceMetaResult { meta }));
    }
}
impl fmt::Display for ListInstanceMetaResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for meta in self.meta.metas.iter() {
            if !first {
                write!(f, "\n")?;
            };
            write!(f, "key: {}, value: {}", meta.key, meta.value)?;
            first = false;
        }
        Ok(())
    }
}

impl VaultCommmand for ListInstanceMeta {
    fn execute(&self) -> CommandResult {
        // Get a handle on the experiment's metadata.
        let handle = ExperimentMetaCollectionHandle::new(self.directory.as_str())?;
        ListInstanceMetaResult::new(handle.meta)
    }
}

/// RegisterExperiment represents a vault register command.
struct RegisterExperiment {
    directory: Option<String>,
    experiment: String,
}

impl VaultCommmand for RegisterExperiment {
    fn execute(&self) -> CommandResult {
        // Open the vault.
        let mut vault = Vault::new(self.directory.clone())?;
        vault.register_experiment(self.experiment.as_str())?;
        VaultUnit::new()
    }
}

/// StoreExperimentInstance represents a vault add command.
struct StoreExperimentInstance {
    directory: Option<String>,
    experiment: String,
    instance: String,
}

impl VaultCommmand for StoreExperimentInstance {
    fn execute(&self) -> CommandResult {
        // Open the vault.
        let mut vault = Vault::new(self.directory.clone())?;
        // Generate a new ID for the instance.
        let id = uuid::Uuid::new_v4().to_string();
        // Get the target experiment from the vault.
        let exp = vault.get_experiment_err(self.experiment.as_str())?;
        // Take the instance's metadata and turn it into a JSON blob.
        let instance = ExperimentMetaCollectionHandle::new(self.instance.as_str())?;
        let mut builder = serde_json::Map::new();
        for meta in instance.meta.metas.iter() {
            // TODO (rohany): A followup is to try and parse more "structured" JSON objects here.
            let value = serde_json::Value::String(meta.value.clone());
            builder.insert(meta.key.clone(), value);
        }
        // TODO (rohany): Don't panic and unwrap here.
        let json = serde_json::to_string(&builder).unwrap();

        // Add the instance to the database.
        let _ = util::wrap_db_error(
            diesel::insert_into(experiment_instance::table)
                .values(schema::ExperimentInstanceInsert {
                    experiment_id: exp.id,
                    uuid: id.as_str(),
                    meta_text: json.as_str(),
                })
                .execute(&mut vault.conn),
        )?;
        // Store the instance's data in the vault.
        vault.store_instance(self.instance.as_str(), id.as_str())?;
        VaultUnit::new()
    }
}

#[derive(any_derive::AsAny)]
/// InstanceListResult is a result type that can be used by commands
/// that return a list of instance directories.
struct InstanceListResult {
    directory: Option<Vec<String>>,
}
impl VaultResult for InstanceListResult {}
impl InstanceListResult {
    fn new(directory: Option<Vec<String>>) -> CommandResult {
        Ok(Box::new(InstanceListResult { directory }))
    }
}
impl fmt::Display for InstanceListResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.directory.as_ref() {
            Some(d) => {
                let mut first = true;
                for s in d.iter() {
                    if !first {
                        write!(f, "\n")?;
                    }
                    write!(f, "{}", s)?;
                    first = false;
                }
                Ok(())
            }
            None => write!(f, "No stored instances!"),
        }
    }
}

/// GetLatestInstance represents a vault get-latest command.
struct GetLatestInstance {
    directory: Option<String>,
    experiment: String,
}

impl VaultCommmand for GetLatestInstance {
    fn execute(&self) -> CommandResult {
        // Open the vault.
        let vault = Vault::new(self.directory.clone())?;
        // Get the experiment from the vault.
        let experiment = vault.get_experiment_err(self.experiment.as_str())?;
        // Get the instance with the largest ID and an experiment_id matching the
        // retrieved experiment.
        let res = util::wrap_db_error(
            experiment_instance::table
                .group_by(experiment_instance::experiment_id)
                .filter(experiment_instance::experiment_id.eq(experiment.id))
                .select((experiment_instance::uuid, diesel::dsl::sql("max(id)")))
                .first::<(String, i32)>(&vault.conn)
                .optional(),
        )?;
        let instance_id = match res {
            None => return InstanceListResult::new(None),
            Some((id, _)) => id,
        };
        let path = vault.make_instance_path(&instance_id);
        InstanceListResult::new(Some(vec![path]))
    }
}

/// QueryInstances represents a vault query command.
struct QueryInstances {
    directory: Option<String>,
    experiment: String,
    query: String,
}

impl VaultCommmand for QueryInstances {
    fn execute(&self) -> CommandResult {
        // Open the vault.
        let vault = Vault::new(self.directory.clone())?;
        // Get the experiment from the vault.
        let experiment = vault.get_experiment_err(self.experiment.as_str())?;
        // Get the instances that the user requests.
        #[derive(QueryableByName)]
        struct QueryResult {
            #[sql_type = "Text"]
            uuid: String,
        }
        // TODO (rohany): Should this query move into the db::schema module?
        // TODO (rohany): Think about how we can avoid directly writing SQL as
        //  part of the user's query.
        // TODO (rohany): Think about how to avoid query injection attacks.
        let instances: Vec<QueryResult> = util::wrap_db_error(
            diesel::sql_query(format!(
                "
SELECT
    uuid
FROM
    (
        SELECT
            id, experiment_id, uuid, json(meta_text) AS meta
        FROM
            experiment_instance
        WHERE
            {}
        ORDER BY
            id
    )
WHERE
    experiment_id = ?;
",
                self.query
            ))
            .bind::<Integer, _>(experiment.id)
            .load(&vault.conn),
        )?;
        let mut dirs = Vec::new();
        dirs.reserve(instances.len());
        for instance in instances.iter() {
            dirs.push(vault.make_instance_path(&instance.uuid))
        }
        InstanceListResult::new(Some(dirs))
    }
}

// Meta contains information that is serialized into the base directory of a
// Vault instance.
#[derive(Serialize, Deserialize, Debug)]
struct Meta {
    /// categories is a list of registered experiments.
    categories: Vec<Experiment>,
    // Is there a higher level version object we can use here?
    version: String,
}

// TODO (rohany): Comment this.
// TODO (rohany): We need to start condensing these errors. Look at existing error types
//  for inspiration.
#[derive(Debug)]
pub enum VaultError {
    DatabaseError(diesel::result::Error),
    DuplicateExperimentError(String),
    UnknownExperimentError(String),
    UnsetInstanceDirectory,
    JSONError(serde_json::Error),
    #[allow(dead_code)]
    InvalidDirectory(String),
    #[allow(dead_code)]
    InvalidInstanceDirectory(String),
    InvalidJSONError(String),
    #[allow(dead_code)]
    Unimplemented(String),
    // It is unfortunate to have both of the below.
    IOError(io::Error),
    IOStringError(String),
    SerializationError(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::DatabaseError(e) => write!(f, "Database error: {}", e),
            VaultError::DuplicateExperimentError(s) => write!(f, "experiment {} already exists", s),
            VaultError::UnknownExperimentError(s) => write!(f, "unknown experiment {}", s),
            VaultError::Unimplemented(s) => write!(f, "unimplemented command: {:?}", s),
            VaultError::IOError(e) => write!(f, "IO error: {}", e),
            VaultError::IOStringError(s) => write!(f, "IO error: {}", s),
            VaultError::SerializationError(s) => write!(f, "Serialization error: {}", s),
            VaultError::InvalidDirectory(s) => write!(f, "{} is not a valid directory", s),
            VaultError::JSONError(e) => write!(f, "JSON parsing error: {}", e),
            VaultError::InvalidJSONError(s) => write!(f, "Invalid JSON: {}", s),
            VaultError::InvalidInstanceDirectory(_) => write!(f, "ROHANY WRITE A MESSAGE HERE"),
            VaultError::UnsetInstanceDirectory => {
                write!(f, "please set VAULT_DIR in your environment")
            }
        }
    }
}

#[derive(Debug)]
/// FileHandle is a structure to manage access to a target file.
struct FileHandle {
    filename: String,
}

impl FileHandle {
    /// new creates a new FileHandle.
    fn new(filename: &str) -> FileHandle {
        FileHandle {
            filename: filename.to_string(),
        }
    }

    /// file attempts to open the target file and return the underlying fs::File.
    /// It returns None if the file does not exist.
    fn file(&mut self) -> Result<Option<fs::File>> {
        // Try to open the handle's file.
        let file = match fs::File::open(self.filename.as_str()) {
            Ok(f) => f,
            // If an error is returned, see what kind of error we got.
            Err(e) => {
                return match e.kind() {
                    // If the error says that the file doesn't exist, then we just return None.
                    io::ErrorKind::NotFound => Ok(None),
                    // Otherwise, forward the error up the chain.
                    _ => Err(VaultError::IOError(e)),
                };
            }
        };
        Ok(Some(file))
    }

    /// write writes out the input bytes to the target file. It overwrites and truncates
    /// the file it already exists.
    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        // Open up the handle's file to read from.
        let fo = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(self.filename.as_str());
        let mut file = util::wrap_io_error(fo)?;
        let _ = util::wrap_io_error(file.write(bytes))?;
        Ok(())
    }
}

// Constants for metadata filenames.
const EXPERIMENT_META_FILENAME: &str = ".experimentmeta";

// Type definitions for standard users of MetadataHandle.
type ExperimentMetaCollectionHandle = MetadataHandle<ExperimentMetaCollection>;
impl ExperimentMetaCollectionHandle {
    fn new(dir: &str) -> Result<ExperimentMetaCollectionHandle> {
        MetadataHandle::new_from_dir(dir, EXPERIMENT_META_FILENAME)
    }
}

#[derive(Debug)]
/// MetadataHandle is an abstraction for serialized, persistent metadata. It
/// can be used to take a handle on arbitrary serialized metadata present in
/// the file system.
struct MetadataHandle<T> {
    handle: FileHandle,
    meta: T,
}

/// MetadataHandle is implemented for types that are able to be serialized
/// and deserialized, as well as having default constructors.
impl<T> MetadataHandle<T>
where
    T: de::DeserializeOwned + Serialize + Default,
{
    /// new_from_dir creates a MetadataHandle from a parent directory and filename.
    fn new_from_dir(dir: &str, name: &str) -> Result<MetadataHandle<T>> {
        // TODO (rohany): Validate that the input directory actually does exist.
        let path = path::Path::new(dir).join(name);
        let path_str = match path.to_str() {
            Some(s) => s,
            None => panic!("what do we do here?"),
        };
        MetadataHandle::new_from_file(path_str)
    }

    /// new_from_file creates a MetadataHandle from a filename.
    fn new_from_file(filename: &str) -> Result<MetadataHandle<T>> {
        // Create a handle from the input filename.
        let handle = FileHandle::new(filename);
        let mut result = MetadataHandle {
            handle,
            meta: T::default(),
        };
        // Now depending on whether the file exists already, read existing metadata.
        match result.handle.file()? {
            // If we have a file, go ahead and read it.
            // TODO (rohany): Do some validation on the metadata we just read.
            Some(f) => result.meta = vault_serde::deserialize_from_reader(f)?,
            // Otherwise, do nothing.
            None => {}
        };
        Ok(result)
    }

    /// flush writes out the handle's metadata to the file system.
    fn flush(&mut self) -> Result<()> {
        self.handle
            .write(vault_serde::serialize(&self.meta)?.as_bytes())
    }
}

// TODO (rohany): This should maybe go into the cli module.
/// dispatch_command_line takes a cli::Commands and dispatches to the
/// corresponding vault execution code.
pub fn dispatch_command_line(args: cli::Commands) -> CommandResult {
    // We could not do an allocation here and instead just call the trait
    // method in each of the cases, but I wanted to play around with traits.
    let command: Box<dyn VaultCommmand> = match args {
        cli::Commands::AddMeta { directory, meta } => match meta {
            cli::AddMetaKind::KV { key, value } => Box::new(AddExperimentMeta {
                directory,
                meta: AddExperimentMetaArgs::KV { key, value },
            }),
            cli::AddMetaKind::JSON { json } => Box::new(AddExperimentMeta {
                directory,
                meta: AddExperimentMetaArgs::JSON { json },
            }),
        },
        cli::Commands::ListMeta { directory } => Box::new(ListInstanceMeta { directory }),
        cli::Commands::Register { experiment } => Box::new(RegisterExperiment {
            directory: None,
            experiment,
        }),
        cli::Commands::Store {
            experiment,
            instance,
        } => Box::new(StoreExperimentInstance {
            directory: None,
            experiment,
            instance,
        }),
        cli::Commands::GetLatest { experiment } => Box::new(GetLatestInstance {
            directory: None,
            experiment,
        }),
        cli::Commands::Query { experiment, query } => Box::new(QueryInstances {
            directory: None,
            experiment,
            query,
        }),
    };
    command.execute()
}

/// util contains different utility methods.
mod util {
    use crate::vault::{Result, VaultError};
    use diesel;
    use std::any::Any;
    use std::io;

    /// wrap_io_error unwraps an io::Result into a vault::Result.
    pub fn wrap_io_error<T>(r: io::Result<T>) -> Result<T> {
        match r {
            Ok(v) => Ok(v),
            Err(e) => Err(VaultError::IOError(e)),
        }
    }

    // TODO (rohany): Make these functions parametric over the error type too?

    /// wrap_db_error unwraps a diesel::QueryResult into a vault::Result.
    pub fn wrap_db_error<T>(r: diesel::QueryResult<T>) -> Result<T> {
        match r {
            Ok(v) => Ok(v),
            Err(e) => Err(VaultError::DatabaseError(e)),
        }
    }

    // AsAny is a trait to cast a trait object into the dynamic Any type.
    pub trait AsAny {
        fn as_any(&self) -> &dyn Any;
    }
}

/// vault_serde contains utility serialization and deserialization routines.
mod vault_serde {
    use crate::vault::{Result, VaultError};
    use serde::de;
    use serde::{Deserialize, Serialize};
    use std::io;

    /// serialize is a wrapper around a particular serde serialization method
    /// so that we can change which package is used easily.
    pub fn serialize<T>(value: &T) -> Result<String>
    where
        T: ?Sized + Serialize,
    {
        match serde_json::to_string(value) {
            Ok(s) => Ok(s),
            Err(e) => Err(VaultError::SerializationError(e.to_string())),
        }
    }

    /// deserialize is a wrapper around a particular serde deserialization
    /// method so that we change which package is used easily.
    #[allow(dead_code)]
    pub fn deserialize<'a, T>(s: &'a str) -> Result<T>
    where
        T: ?Sized + Deserialize<'a>,
    {
        match serde_json::from_str(s) {
            Ok(v) => Ok(v),
            Err(e) => Err(VaultError::SerializationError(e.to_string())),
        }
    }

    /// deserialize_from_reader is a wrapper around a particular serde deserialization
    /// method so that we change which package is used easily.
    pub fn deserialize_from_reader<T, R>(r: R) -> Result<T>
    where
        T: ?Sized + de::DeserializeOwned,
        R: io::Read,
    {
        match serde_json::from_reader(r) {
            Ok(v) => Ok(v),
            Err(e) => Err(VaultError::SerializationError(e.to_string())),
        }
    }
}

#[cfg(test)]
mod testutils {
    use std::fs;
    use std::io::Read;

    pub fn assert_file_contents(f: fs::File, expected: &str) {
        let mut f = f;
        let mut contents = String::new();
        f.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, expected);
    }
}

#[cfg(test)]
mod store {
    use crate::vault::db::schema;
    use crate::vault::db::schema::{experiment, experiment_instance};
    use crate::vault::testutils::assert_file_contents;
    use crate::vault::{
        AddExperimentMeta, AddExperimentMetaArgs, RegisterExperiment, StoreExperimentInstance,
        Vault, VaultCommmand, VaultError,
    };
    use diesel::{QueryDsl, RunQueryDsl};
    use std::fs;
    use std::io::Write;

    #[test]
    fn basic() {
        // Create the vault directory.
        let dir = tempdir::TempDir::new("vault").unwrap();
        let vault_path = dir.path().to_str().unwrap().to_string();

        // We should get an error if we attempt to store into an experiment
        // that doesn't exist yet.
        let res = StoreExperimentInstance {
            directory: Some(vault_path.clone()),
            experiment: "fake".to_string(),
            instance: "fake".to_string(),
        }
        .execute();
        // TODO (rohany): Make a "require error wrapper".
        match res {
            Ok(_) => panic!("Expected error, found success!"),
            Err(VaultError::UnknownExperimentError(_)) => {}
            Err(e) => panic!("Unexpected error: {}", e),
        }

        // Register an experiment.
        RegisterExperiment {
            directory: Some(vault_path.clone()),
            experiment: "exp".to_string(),
        }
        .execute()
        .unwrap();

        // Create an experiment directory.
        let dir = tempdir::TempDir::new("experiment").unwrap();
        let exp_path = dir.path().to_str().unwrap().to_string();

        // Add some data into the experiment.
        let mut f = fs::File::create(format!("{}/a", &exp_path)).unwrap();
        f.write("hello".as_bytes()).unwrap();
        // Add a directory with another file.
        fs::DirBuilder::new()
            .create(format!("{}/dir", &exp_path))
            .unwrap();
        // Add another file into the directory.
        let mut f = fs::File::create(format!("{}/dir/b", &exp_path)).unwrap();
        f.write("hello2".as_bytes()).unwrap();

        // Add some meta-data to this experiment directory.
        AddExperimentMeta {
            directory: exp_path.clone(),
            meta: AddExperimentMetaArgs::KV {
                key: "key".to_string(),
                value: "value".to_string(),
            },
        }
        .execute()
        .unwrap();

        // Finally, store this experiment into the vault.
        StoreExperimentInstance {
            directory: Some(vault_path.clone()),
            experiment: "exp".to_string(),
            instance: exp_path.clone(),
        }
        .execute()
        .unwrap();

        // Verify that the experiment structure is as expected.
        let vault = Vault::new_from_dir(vault_path.as_str()).unwrap();
        let instances = vault.get_experiment_instances("exp").unwrap();
        // There should only be one instance.
        assert_eq!(instances.len(), 1);
        let instance = instances[0].as_str();

        // // We should find the files that placed in the experiment.
        let path = format!("{}/{}/a", vault.base_dir, instance);
        assert_file_contents(fs::File::open(path).unwrap(), "hello");
        let path = format!("{}/{}/dir/b", vault.base_dir, instance);
        assert_file_contents(fs::File::open(path).unwrap(), "hello2");

        // The original experiment directory should not have been altered.
        let path = format!("{}/a", &exp_path);
        assert_file_contents(fs::File::open(path).unwrap(), "hello");
        let path = format!("{}/dir/b", &exp_path);
        assert_file_contents(fs::File::open(path).unwrap(), "hello2");

        let rows: Vec<(schema::Experiment, schema::ExperimentInstance)> = experiment::table
            .inner_join(experiment_instance::table)
            .load(&vault.conn)
            .unwrap();
        assert_eq!(rows.len(), 1);
        let (exp, inst) = &rows[0];
        // Double check that SQLite is upholding FK relationships, because why not?
        assert_eq!(exp.id, inst.experiment_id);
        assert_eq!(exp.name, "exp");
        assert_eq!(inst.meta_text, r#"{"key":"value"}"#);
    }
}

#[cfg(test)]
mod datadriven_tests {
    use super::*;
    use datadriven::walk;
    use std::collections::HashMap;
    use std::io::Read;

    const INSTANCE_FILE_NAME: &str = "instance_name";

    fn result_to_string(r: CommandResult) -> String {
        match r {
            Ok(f) => match f.to_string().as_str() {
                "" => "".to_string(),
                s => format!("{}\n", s),
            },
            Err(e) => format!("Error: {}\n", e.to_string()),
        }
    }

    fn instance_list_result_to_string(b: Box<dyn VaultResult>) -> String {
        // Downcast the result into the expected struct.
        let latest_result = b.as_any().downcast_ref::<InstanceListResult>().unwrap();
        match &latest_result.directory {
            // If there isn't a latest instance, then there's nothing to do.
            None => result_to_string(Ok(b)),
            Some(dirs) => {
                let mut result = String::new();
                for dir in dirs.iter() {
                    // s is a path to some directory. Let's read out the name of the instance
                    // that this directory contains to see what instance is actually here.
                    let p = path::Path::new(dir).join(INSTANCE_FILE_NAME);
                    let mut f = fs::File::open(p).unwrap();
                    let mut name = String::new();
                    let _ = f.read_to_string(&mut name).unwrap();
                    result.push_str(format!("{}\n", name).as_str())
                }
                result
            }
        }
    }

    #[test]
    fn run() {
        walk("test/testdata/vault/commands", |f| {
            // In order to avoid temporary directories from being cleaned up
            // when their references go out of scope, we collect them all so that
            // they can be dropped at the end of the test.
            let mut active_dirs = vec![];
            let mut vault_path = None;
            let mut instances = HashMap::new();
            f.run(|test_case| -> String {
                match test_case.directive.as_str() {
                    "new-vault" => {
                        let dir = tempdir::TempDir::new("vault").unwrap();
                        vault_path = Some(dir.path().to_str().unwrap().to_string());
                        active_dirs.push(dir);
                        "".to_string()
                    }
                    "new-instance" => {
                        let dir = tempdir::TempDir::new("experiment").unwrap();
                        let path = dir.path().to_str().unwrap().to_string();
                        let name = test_case.args["name"][0].clone();
                        instances.insert(name.clone(), path);
                        // Drop a file into this directory with the name of the instance.
                        // This will be used to identify instances later.
                        let mut f = fs::File::create(&dir.path().join(INSTANCE_FILE_NAME)).unwrap();
                        let _ = f.write(name.as_bytes()).unwrap();
                        active_dirs.push(dir);
                        "".to_string()
                    }
                    "add-meta" => {
                        let instance = instances
                            .get(test_case.args["instance"][0].as_str())
                            .unwrap();
                        result_to_string(
                            // If the test case uses JSON, then pull that out of the input. Otherwise,
                            // assume that key-value pairs will be passed in.
                            match test_case.args.get("json") {
                                Some(_) => AddExperimentMeta {
                                    directory: instance.clone(),
                                    meta: AddExperimentMetaArgs::JSON {
                                        json: test_case.input.clone(),
                                    },
                                },
                                None => {
                                    let key = test_case.args["key"][0].clone();
                                    let value = test_case.args["value"][0].clone();
                                    AddExperimentMeta {
                                        directory: instance.clone(),
                                        meta: AddExperimentMetaArgs::KV { key, value },
                                    }
                                }
                            }
                            .execute(),
                        )
                    }
                    "list-meta" => {
                        let instance = instances
                            .get(test_case.args["instance"][0].as_str())
                            .unwrap();
                        result_to_string(
                            ListInstanceMeta {
                                directory: instance.clone(),
                            }
                            .execute(),
                        )
                    }
                    "get-latest" => {
                        let experiment = test_case.args["experiment"][0].clone();
                        // Execute the get-latest instance command.
                        let res = GetLatestInstance {
                            directory: Some(vault_path.as_ref().unwrap().clone()),
                            experiment,
                        }
                        .execute();
                        // Rather than just returning the output, we have to clean up the result
                        // so that the temporary file and instance UUID aren't on display.
                        match res {
                            // If an error was raised, then return that directly.
                            Err(_) => result_to_string(res),
                            // If a result is returned, we have to inspect it.
                            Ok(b) => instance_list_result_to_string(b),
                        }
                    }
                    "register-experiment" => result_to_string(
                        RegisterExperiment {
                            directory: Some(vault_path.as_ref().unwrap().clone()),
                            experiment: test_case.args["name"][0].clone(),
                        }
                        .execute(),
                    ),
                    "store-instance" => {
                        let experiment = test_case.args["experiment"][0].clone();
                        let instance_name = test_case.args["instance"][0].clone();
                        let instance = instances.get(&instance_name).unwrap();
                        result_to_string(
                            StoreExperimentInstance {
                                directory: Some(vault_path.as_ref().unwrap().clone()),
                                experiment,
                                instance: instance.clone(),
                            }
                            .execute(),
                        )
                    }
                    "query" => {
                        let experiment = test_case.args["experiment"][0].clone();
                        // Execute the query instance command.
                        let res = QueryInstances {
                            directory: Some(vault_path.as_ref().unwrap().clone()),
                            experiment,
                            query: test_case.input.clone(),
                        }
                        .execute();
                        // Rather than just returning the output, we have to clean up the result
                        // so that the temporary file and instance UUID aren't on display.
                        match res {
                            // If an error was raised, then return that directly.
                            Err(_) => result_to_string(res),
                            // If a result is returned, we have to inspect it.
                            Ok(b) => instance_list_result_to_string(b),
                        }
                    }
                    _ => panic!("unhandled directive: {}", test_case.directive),
                }
            })
        })
    }
}
