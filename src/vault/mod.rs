use crate::cli;
use fs_extra;
use serde::de;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::{env, fmt, fs, io, path, result};
use uuid;

/// Result is an alias for Result<T, VaultError>. It is used for uniformity
/// among the different vault functions.
pub type Result<T> = result::Result<T, VaultError>;

/// VaultCommand is a common trait that command drivers will implement.
trait VaultCommmand {
    // TODO (rohany): This needs to be parameterized by a trait object that just implements
    //  a "display" trait, so that we can eventually print out the results to stdout, but also
    //  verify that the results are as we expect.
    fn execute(&self) -> Result<()>;
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
    fn new() -> Result<Vault> {
        let base_dir = match env::var_os("VAULT_DIR") {
            Some(dir) => dir,
            None => return Err(VaultError::UnsetInstanceDirectory),
        };
        Vault::new_from_dir(base_dir.to_str().unwrap())
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
    fn execute(&self) -> Result<()> {
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
        Ok(())
    }
}

/// RegisterExperiment represents a vault register command.
struct RegisterExperiment {
    directory: Option<String>,
    experiment: String,
}

impl VaultCommmand for RegisterExperiment {
    fn execute(&self) -> Result<()> {
        // Open the vault.
        let mut vault = match &self.directory {
            Some(d) => Vault::new_from_dir(d.as_str()),
            None => Vault::new(),
        }?;
        vault.register_experiment(self.experiment.as_str())
    }
}

/// StoreExperimentInstance represents a vault add command.
struct StoreExperimentInstance {
    directory: Option<String>,
    experiment: String,
    instance: String,
}

impl VaultCommmand for StoreExperimentInstance {
    fn execute(&self) -> Result<()> {
        // Open the vault.
        // TODO (rohany): Make the new_from_dir take in an option!
        let mut vault = match &self.directory {
            Some(d) => Vault::new_from_dir(d.as_str()),
            None => Vault::new(),
        }?;
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

        Ok(())
    }
}

/// GetLatestInstance represents a vault get-latest command.
struct GetLatestInstance {
    directory: Option<String>,
    experiment: String,
}

impl VaultCommmand for GetLatestInstance {
    fn execute(&self) -> Result<()> {
        // In an ideal world, we should reduce this to a call of Query.

        // Open the vault.
        let mut vault = match &self.directory {
            Some(d) => Vault::new_from_dir(d.as_str()),
            None => Vault::new(),
        }?;

        // Open up the target experiment.
        let (experiment, handle) = vault.open_experiment(self.experiment.as_str())?;

        let id = match experiment.latest_instance_id {
            None => {
                // TODO (rohany): We'll want to do something else here.
                println!("No instances recorded for this experiment!");
                return Ok(());
            }
            Some(id) => id,
        };

        let path = format!("{}/{}", handle.directory, id.to_string());
        println!("path: {}", path);

        Ok(())
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

// Type definitions for standard users of MetadataHandle.
type ExperimentInstanceHandle = MetadataHandle<ExperimentInstance>;
impl ExperimentInstanceHandle {
    fn new(dir: &str) -> Result<ExperimentInstanceHandle> {
        // TODO (rohany): Make this a constant.
        MetadataHandle::new_from_dir(dir, ".instancemeta")
    }
}

type ExperimentMetaCollectionHandle = MetadataHandle<ExperimentMetaCollection>;
impl ExperimentMetaCollectionHandle {
    fn new(dir: &str) -> Result<ExperimentMetaCollectionHandle> {
        // TODO (rohany): Make this a constant.
        MetadataHandle::new_from_dir(dir, ".experimentmeta")
    }
}

type VaultMetaHandle = MetadataHandle<VaultMeta>;
impl VaultMetaHandle {
    fn new(dir: &str) -> Result<VaultMetaHandle> {
        // TODO (rohany): Make this a constant.
        MetadataHandle::new_from_dir(dir, ".vaultmeta")
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
pub fn dispatch_command_line(args: cli::Commands) -> Result<()> {
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
    use std::io;
    /// wrap_io_error unwraps an io::Result into a vault::Result.
    pub fn wrap_io_error<T>(r: io::Result<T>) -> Result<T> {
        match r {
            Ok(v) => Ok(v),
            Err(e) => Err(VaultError::IOError(e)),
        }
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
mod add_meta {
    use crate::vault::{
        AddExperimentMeta, ExperimentMeta, ExperimentMetaCollectionHandle, VaultCommmand,
    };
    use tempdir;

    // Tests for the add-meta command driver.
    #[test]
    fn basic() {
        let dir = tempdir::TempDir::new("add-meta").unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        // Add some metadata to an empty experiment.
        let _ = AddExperimentMeta {
            directory: path.clone(),
            key: "key".parse().unwrap(),
            value: "value".parse().unwrap(),
        }
        .execute()
        .unwrap();
        // We should find this data now in the metadata.
        let handle = ExperimentMetaCollectionHandle::new(path.as_str()).unwrap();
        assert_eq!(
            handle.meta.metas,
            vec![ExperimentMeta {
                key: "key".to_string(),
                value: "value".to_string()
            }]
        );

        // If we add another key-value then we should find it as well.
        let _ = AddExperimentMeta {
            directory: path.clone(),
            key: "key2".parse().unwrap(),
            value: "value2".parse().unwrap(),
        }
        .execute()
        .unwrap();

        let handle = ExperimentMetaCollectionHandle::new(path.as_str()).unwrap();
        assert_eq!(
            handle.meta.metas,
            vec![
                ExperimentMeta {
                    key: "key".to_string(),
                    value: "value".to_string()
                },
                ExperimentMeta {
                    key: "key2".to_string(),
                    value: "value2".to_string()
                },
            ]
        )
    }

    #[test]
    fn no_duplicates() {
        let dir = tempdir::TempDir::new("add-meta").unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        // Add some metadata to an empty experiment.
        let _ = AddExperimentMeta {
            directory: path.clone(),
            key: "key".parse().unwrap(),
            value: "value".parse().unwrap(),
        }
        .execute()
        .unwrap();
        // Adding the same value again shouldn't result an an error and should
        // overwrite the value in the experiment.
        let _ = AddExperimentMeta {
            directory: path.clone(),
            key: "key".parse().unwrap(),
            value: "value2".parse().unwrap(),
        }
        .execute()
        .unwrap();
        // We should find this data now in the metadata.
        let handle = ExperimentMetaCollectionHandle::new(path.as_str()).unwrap();
        assert_eq!(
            handle.meta.metas,
            vec![ExperimentMeta {
                key: "key".to_string(),
                value: "value2".to_string()
            }]
        );
    }
}

#[cfg(test)]
mod register {
    use crate::vault::{RegisterExperiment, VaultCommmand, VaultError, VaultMetaHandle};

    // Tests for the register command driver.
    #[test]
    fn basic() {
        let dir = tempdir::TempDir::new("register").unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        let _ = RegisterExperiment {
            directory: Some(path.clone()),
            experiment: "exp1".to_string(),
        }
        .execute()
        .unwrap();

        // We should find this experiment registered.
        let handle = VaultMetaHandle::new(path.as_str()).unwrap();
        assert!(handle.meta.contains_experiment("exp1"));

        // Add another experiment, and expect to find it.
        let _ = RegisterExperiment {
            directory: Some(path.clone()),
            experiment: "exp2".to_string(),
        }
        .execute()
        .unwrap();

        let handle = VaultMetaHandle::new(path.as_str()).unwrap();
        assert!(handle.meta.contains_experiment("exp1"));
        assert!(handle.meta.contains_experiment("exp2"));
    }

    #[test]
    fn no_duplicate_experiments() {
        let dir = tempdir::TempDir::new("register").unwrap();
        let path = dir.path().to_str().unwrap().to_string();
        let _ = RegisterExperiment {
            directory: Some(path.clone()),
            experiment: "exp".to_string(),
        }
        .execute()
        .unwrap();
        // We should get an error attempting to register the same experiment twice.
        let res = RegisterExperiment {
            directory: Some(path.clone()),
            experiment: "exp".to_string(),
        }
        .execute();
        match res {
            Ok(_) => panic!("Expected error, found success!"),
            Err(VaultError::DuplicateExperimentError(_)) => {}
            Err(e) => panic!("Unexpected error: {}", e),
        }
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
mod get_latest {
    use crate::vault::{
        GetLatestInstance, RegisterExperiment, StoreExperimentInstance, VaultCommmand, VaultError,
    };

    #[test]
    fn basic() {
        // Create the vault directory.
        let dir = tempdir::TempDir::new("vault").unwrap();
        let vault_path = dir.path().to_str().unwrap().to_string();
        // Create an experiment directory.
        let dir = tempdir::TempDir::new("vault").unwrap();
        let exp_path = dir.path().to_str().unwrap().to_string();

        // Register an experiment.
        RegisterExperiment {
            directory: Some(vault_path.clone()),
            experiment: "exp".to_string(),
        }
        .execute()
        .unwrap();

        // Getting the latest from an invalid experiment should error.
        let res = GetLatestInstance {
            directory: Some(vault_path.clone()),
            experiment: "ex".to_string(),
        }
        .execute();
        match res {
            Ok(_) => panic!("Expected error, found success!"),
            Err(VaultError::UnknownExperimentError(_)) => {}
            Err(e) => panic!("Unexpected error: {}", e),
        }

        // Getting the latest from an experiment with no instances should be empty.
        GetLatestInstance {
            directory: Some(vault_path.clone()),
            experiment: "exp".to_string(),
        }
        .execute()
        .unwrap();

        // Now store an experiment instance.
        StoreExperimentInstance {
            directory: Some(vault_path.clone()),
            experiment: "exp".to_string(),
            instance: exp_path.clone(),
        }
        .execute()
        .unwrap();

        // Getting the latest instance should "work" now.
        GetLatestInstance {
            directory: Some(vault_path.clone()),
            experiment: "exp".to_string(),
        }
        .execute()
        .unwrap();
    }
}
