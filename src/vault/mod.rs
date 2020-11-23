use crate::cli;
use any_derive;
use fs_extra;
use serde::de;
use serde::export::Formatter;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::io::Write;
use std::{env, fmt, fs, io, path, result};
use uuid;

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
    /// metadata_handle is a handle pointing to the instance's metadata.
    metadata_handle: VaultMetaHandle,
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
            metadata_handle: VaultMetaHandle::new(instance_dir)?,
        })
    }

    /// open_experiment opens a mutable handle to a target experiment.
    fn open_experiment(
        &mut self,
        experiment_name: &str,
    ) -> Result<(&mut Experiment, ExperimentHandle)> {
        let experiment = match self
            .metadata_handle
            .meta
            .get_experiment_mut(experiment_name)
        {
            None => {
                return Err(VaultError::UnknownExperimentError(
                    experiment_name.to_string(),
                ))
            }
            Some(e) => e,
        };
        // TODO (rohany): We might want to generate UUID's or something for experiments
        //  so that they are resilient to name changes.
        let experiment_dir = format!("{}/{}", self.base_dir, experiment_name);
        let handle = ExperimentHandle::new(experiment_dir.as_str())?;
        Ok((experiment, handle))
    }

    /// register_experiment registers a new experiment in the vault.
    fn register_experiment(&mut self, experiment: &str) -> Result<()> {
        // See if this experiment exists already.
        match self
            .metadata_handle
            .meta
            .experiments
            .iter()
            .find(|e| e.name == experiment)
        {
            Some(_) => {
                return Err(VaultError::DuplicateExperimentError(
                    experiment.to_string().clone(),
                ))
            }
            None => {
                // Add the new experiment to the metadata.
                self.metadata_handle.meta.experiments.push(Experiment {
                    name: experiment.to_string(),
                    latest_instance_id: None,
                });
                // Flush the changes.
                self.metadata_handle.flush()?;
            }
        };
        // Create a directory for this experiment in the vault.
        let dir_name = format!("{}/{}", self.base_dir, experiment);
        util::wrap_io_error(fs::create_dir(path::Path::new(&dir_name)))?;
        Ok(())
    }
}

#[derive(Debug)]
/// AddExperimentMeta represents a vault add-meta command.
struct AddExperimentMeta {
    /// directory is the directory corresponding to an experiment instance.
    directory: String,
    // TODO (rohany): We could probably just make these &str's.
    key: String,
    value: String,
}

impl VaultCommmand for AddExperimentMeta {
    fn execute(&self) -> CommandResult {
        // Get a handle on the experiment's metadata.
        let mut handle = ExperimentMetaCollectionHandle::new(self.directory.as_str())?;
        // See if this key exists already.
        match handle
            .meta
            .metas
            .iter_mut()
            .filter(|m| m.key.as_str() == self.key.as_str())
            .next()
        {
            // If the key exists already, replace the value with the new one.
            Some(v) => v.value = self.value.clone(),
            // Otherwise, add the new key.
            None => handle.meta.metas.push(ExperimentMeta {
                key: self.key.clone(),
                value: self.value.clone(),
            }),
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
        // Ensure that this experiment is registered in the vault.
        if !vault
            .metadata_handle
            .meta
            .contains_experiment(self.experiment.as_str())
        {
            return Err(VaultError::UnknownExperimentError(self.experiment.clone()));
        };
        // Read any metadata about the experiment.
        let instance = ExperimentMetaCollectionHandle::new(self.instance.as_str())?;

        // Open up the experiment directory.
        let (mut experiment, mut experiment_handle) =
            vault.open_experiment(self.experiment.as_str())?;

        // Copy in the target experiment.
        let id = uuid::Uuid::new_v4();
        experiment_handle.store(self.instance.as_str(), &id.to_string().as_str())?;

        // Write out the instance's metadata.
        let mut instance_meta = experiment_handle.open_instance_meta(&id)?;
        instance_meta.meta = ExperimentInstance {
            name: self.experiment.clone(),
            meta: instance.meta.clone(),
            timestamp: "TODO (rohany): fill this out".to_string(),
            id,
        };
        instance_meta.flush()?;

        // Update the pointer to the latest instance for this experiment.
        experiment.latest_instance_id = Some(id);
        vault.metadata_handle.flush()?;

        VaultUnit::new()
    }
}

/// GetLatestInstance represents a vault get-latest command.
struct GetLatestInstance {
    directory: Option<String>,
    experiment: String,
}

#[derive(any_derive::AsAny)]
/// GetLatestInstanceResult is the output of a GetLatestInstance command.
struct GetLatestInstanceResult {
    directory: Option<String>,
}
impl VaultResult for GetLatestInstanceResult {}
impl GetLatestInstanceResult {
    fn new(directory: Option<String>) -> CommandResult {
        Ok(Box::new(GetLatestInstanceResult { directory }))
    }
}

impl fmt::Display for GetLatestInstanceResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.directory.as_ref() {
            Some(d) => write!(f, "{}", d),
            None => write!(f, "No stored instances!"),
        }
    }
}

impl VaultCommmand for GetLatestInstance {
    fn execute(&self) -> CommandResult {
        // In an ideal world, we should reduce this to a call of Query.

        // Open the vault.
        let mut vault = Vault::new(self.directory.clone())?;

        // Open up the target experiment.
        let (experiment, handle) = vault.open_experiment(self.experiment.as_str())?;

        let id = match experiment.latest_instance_id {
            None => return GetLatestInstanceResult::new(None),
            Some(id) => id,
        };

        let path = format!("{}/{}", handle.directory, id.to_string());
        GetLatestInstanceResult::new(Some(path))
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
    DuplicateExperimentError(String),
    UnknownExperimentError(String),
    UnsetInstanceDirectory,
    InvalidDirectory(String),
    #[allow(dead_code)]
    InvalidInstanceDirectory(String),
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
            VaultError::DuplicateExperimentError(s) => write!(f, "experiment {} already exists", s),
            VaultError::UnknownExperimentError(s) => write!(f, "unknown experiment {}", s),
            VaultError::Unimplemented(s) => write!(f, "unimplemented command: {:?}", s),
            VaultError::IOError(e) => write!(f, "IO error: {}", e),
            VaultError::IOStringError(s) => write!(f, "IO error: {}", s),
            VaultError::SerializationError(s) => write!(f, "Serialization error: {}", s),
            VaultError::InvalidDirectory(s) => write!(f, "{} is not a valid directory", s),
            VaultError::InvalidInstanceDirectory(_) => write!(f, "ROHANY WRITE A MESSAGE HERE"),
            VaultError::UnsetInstanceDirectory => {
                write!(f, "please set VAULT_DIR in your environment")
            }
        }
    }
}

/// ExperimentHandle is a handle to an Experiment directory.
struct ExperimentHandle {
    directory: String,
}

impl ExperimentHandle {
    /// new creates an ExperimentHandle from an input directory.
    fn new(directory: &str) -> Result<ExperimentHandle> {
        match path::Path::new(directory).is_dir() {
            true => Ok(ExperimentHandle {
                directory: directory.to_string(),
            }),
            false => Err(VaultError::InvalidDirectory(directory.to_string())),
        }
    }

    /// store stores the target experiment instance directory into the experiment
    /// under the target name. This operation performs a recursive copy from the
    /// target directory into the experiment's directory.
    fn store(&mut self, target: &str, name: &str) -> Result<()> {
        // Create options for the copy.
        let mut options = fs_extra::dir::CopyOptions::new();
        // copy_inside allows us to copy the directory into a new name.
        options.copy_inside = true;
        let destination = format!("{}/{}", &self.directory, name);
        match fs_extra::dir::copy(target, destination, &options) {
            Ok(_) => Ok(()),
            Err(e) => Err(VaultError::IOStringError(e.to_string())),
        }
    }

    /// open_instance_meta opens a handle to an experiment instance with the input ID.
    fn open_instance_meta(&mut self, id: &uuid::Uuid) -> Result<ExperimentInstanceHandle> {
        // TODO (rohany): All of these key construction routines should become functions.
        let dir = format!("{}/{}", &self.directory, id.to_string());
        ExperimentInstanceHandle::new(dir.as_str())
    }

    #[allow(dead_code)]
    /// iter_instances returns an Iterator to all available instances of the experiment.
    fn iter_instances(&mut self) -> Result<InstanceIterator> {
        let dirs = util::wrap_io_error(fs::read_dir(&self.directory))?;
        Ok(InstanceIterator { dirs })
    }
}

/// InstanceIterator is an Iterator through all instances in an experiment.
struct InstanceIterator {
    dirs: fs::ReadDir,
}

impl Iterator for InstanceIterator {
    type Item = Result<ExperimentInstanceHandle>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.dirs.next() {
            None => None,
            Some(Err(e)) => Some(Err(VaultError::IOError(e))),
            Some(Ok(dir)) => Some(ExperimentInstanceHandle::new(dir.path().to_str().unwrap())),
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
const INSTANCE_META_FILENAME: &str = ".instancemeta";
const EXPERIMENT_META_FILENAME: &str = ".experimentmeta";
const VAULT_META_FILENAME: &str = ".vaultmeta";

// Type definitions for standard users of MetadataHandle.
type ExperimentInstanceHandle = MetadataHandle<ExperimentInstance>;
impl ExperimentInstanceHandle {
    fn new(dir: &str) -> Result<ExperimentInstanceHandle> {
        MetadataHandle::new_from_dir(dir, INSTANCE_META_FILENAME)
    }
}

type ExperimentMetaCollectionHandle = MetadataHandle<ExperimentMetaCollection>;
impl ExperimentMetaCollectionHandle {
    fn new(dir: &str) -> Result<ExperimentMetaCollectionHandle> {
        MetadataHandle::new_from_dir(dir, EXPERIMENT_META_FILENAME)
    }
}

type VaultMetaHandle = MetadataHandle<VaultMeta>;
impl VaultMetaHandle {
    fn new(dir: &str) -> Result<VaultMetaHandle> {
        MetadataHandle::new_from_dir(dir, VAULT_META_FILENAME)
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

#[derive(Serialize, Deserialize, Default, Debug)]
/// VaultMeta is the serialized metadata that corresponds to a vault instance.
struct VaultMeta {
    experiments: Vec<Experiment>,
    // TODO (rohany): Include a version here?
    // TODO (rohany): Include a "modified" timestamp here.
}

impl VaultMeta {
    /// contains_experiment returns whether a given experiment is registered in this meta.
    fn contains_experiment(&self, experiment: &str) -> bool {
        match self.get_experiment(experiment) {
            Some(_) => true,
            None => false,
        }
    }

    /// get_experiment returns a reference to an experiment's metadata from the vault.
    fn get_experiment(&self, experiment: &str) -> Option<&Experiment> {
        self.experiments
            .iter()
            .find(|e| e.name.as_str() == experiment)
    }

    /// get_experiment_mut returns a mutable reference to an experiment's metadata from the vault.
    fn get_experiment_mut(&mut self, experiment: &str) -> Option<&mut Experiment> {
        self.experiments
            .iter_mut()
            .find(|e| e.name.as_str() == experiment)
    }
}

// TODO (rohany): This should maybe go into the cli module.
/// dispatch_command_line takes a cli::Commands and dispatches to the
/// corresponding vault execution code.
pub fn dispatch_command_line(args: cli::Commands) -> CommandResult {
    // We could not do an allocation here and instead just call the trait
    // method in each of the cases, but I wanted to play around with traits.
    let command: Box<dyn VaultCommmand> = match args {
        cli::Commands::AddMeta {
            directory,
            key,
            value,
        } => Box::new(AddExperimentMeta {
            directory,
            key,
            value,
        }),
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
        cmd => panic!("unhandled command {:?}", cmd),
    };
    command.execute()
}

/// util contains different utility methods.
mod util {
    use crate::vault::{Result, VaultError};
    use std::any::Any;
    use std::io;

    /// wrap_io_error unwraps an io::Result into a vault::Result.
    pub fn wrap_io_error<T>(r: io::Result<T>) -> Result<T> {
        match r {
            Ok(v) => Ok(v),
            Err(e) => Err(VaultError::IOError(e)),
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
    use crate::vault::testutils::assert_file_contents;
    use crate::vault::{
        AddExperimentMeta, RegisterExperiment, StoreExperimentInstance, Vault, VaultCommmand,
        VaultError,
    };
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
            key: "key".to_string(),
            value: "value".to_string(),
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

        // Start to verify that the experiment structure is as expected.
        let mut vault = Vault::new_from_dir(vault_path.as_str()).unwrap();
        let (_, mut experiment) = vault.open_experiment("exp").unwrap();
        let mut instances = experiment.iter_instances().unwrap();
        let instance = instances.next().unwrap().unwrap();
        // There should only be one instance, so another call to next must fail.
        match instances.next() {
            None => {}
            Some(_) => panic!("expected None, found Some"),
        };
        // The instance should have the key-value that we want.
        assert_eq!(instance.meta.name, "exp");
        assert_eq!(
            instance
                .meta
                .meta
                .metas
                .iter()
                .find(|m| m.key == "key")
                .unwrap()
                .value,
            "value"
        );
        // We should find the files that placed in the experiment.
        let path = format!("{}/{}/a", experiment.directory, &instance.meta.id);
        assert_file_contents(fs::File::open(path).unwrap(), "hello");
        let path = format!("{}/{}/dir/b", experiment.directory, &instance.meta.id);
        assert_file_contents(fs::File::open(path).unwrap(), "hello2");

        // The original experiment directory should not have been altered.
        let path = format!("{}/a", &exp_path);
        assert_file_contents(fs::File::open(path).unwrap(), "hello");
        let path = format!("{}/dir/b", &exp_path);
        assert_file_contents(fs::File::open(path).unwrap(), "hello2");
    }
}

#[cfg(test)]
mod datadriven_tests {
    use super::*;
    use datadriven::walk;
    use std::collections::HashMap;
    use std::io::Read;

    fn result_to_string(r: CommandResult) -> String {
        match r {
            Ok(f) => match f.to_string().as_str() {
                "" => "".to_string(),
                s => format!("{}\n", s),
            },
            Err(e) => format!("Error: {}\n", e.to_string()),
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
            let instance_file_name = "instance_name";
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
                        let mut f = fs::File::create(&dir.path().join(instance_file_name)).unwrap();
                        let _ = f.write(name.as_bytes()).unwrap();
                        active_dirs.push(dir);
                        "".to_string()
                    }
                    "add-meta" => {
                        let instance = instances
                            .get(test_case.args["instance"][0].as_str())
                            .unwrap();
                        let key = test_case.args["key"][0].clone();
                        let value = test_case.args["value"][0].clone();
                        result_to_string(
                            AddExperimentMeta {
                                directory: instance.clone(),
                                key,
                                value,
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
                            Ok(b) => {
                                // Downcast the result into the expected struct.
                                let latest_result = b
                                    .as_any()
                                    .downcast_ref::<GetLatestInstanceResult>()
                                    .unwrap();
                                match &latest_result.directory {
                                    // If there isn't a latest instance, then there's nothing to do.
                                    None => result_to_string(Ok(b)),
                                    Some(s) => {
                                        // s is a path to some directory. Let's read out the name of the instance
                                        // that this directory contains to see what instance is actually here.
                                        let p = path::Path::new(s).join(instance_file_name);
                                        let mut f = fs::File::open(p).unwrap();
                                        let mut name = String::new();
                                        let _ = f.read_to_string(&mut name).unwrap();
                                        format!("{}\n", name)
                                    }
                                }
                            }
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
                    _ => panic!("unhandled directive: {}", test_case.directive),
                }
            })
        })
    }
}
