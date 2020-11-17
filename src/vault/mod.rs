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

trait VaultCommmand {
    fn execute(&self) -> Result<()>;
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Experiment {
    pub name: String, // Maybe this has a pointer to the latest version of an experiment?
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Default)]
struct ExperimentInstance {
    // Name of the experiment this is part of?
    name: String,
    meta: ExperimentMetaCollection,
    // TODO (rohany): Figure out what concrete type we'll use here.
    timestamp: String,
    id: uuid::Uuid,
}

// TODO (rohany): Comment this
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
struct ExperimentMetaCollection {
    metas: Vec<ExperimentMeta>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
struct ExperimentMeta {
    key: String,
    value: String,
}

struct Vault {
    #[allow(dead_code)]
    base_dir: String,
    metadata_handle: VaultMetaHandle,
}

impl Vault {
    fn new() -> Result<Vault> {
        let base_dir = match env::var_os("VAULT_DIR") {
            Some(dir) => dir,
            None => return Err(VaultError::UnsetInstanceDirectory),
        };
        Vault::new_from_dir(base_dir.to_str().unwrap())
    }
    fn new_from_dir(instance_dir: &str) -> Result<Vault> {
        // TODO (rohany): Perform some validation on the Vault instance.
        Ok(Vault {
            base_dir: instance_dir.to_string(),
            metadata_handle: VaultMetaHandle::new(instance_dir)?,
        })
    }
    fn open_experiment(&mut self, experiment: &str) -> Result<ExperimentHandle> {
        if !self.metadata_handle.meta.contains_experiment(experiment) {
            return Err(VaultError::UnknownExperimentError(experiment.to_string()));
        };
        // TODO (rohany): We might want to generate UUID's or something for experiments
        //  so that they are resilient to name changes.
        let experiment_dir = format!("{}/{}", self.base_dir, experiment);
        ExperimentHandle::new(experiment_dir.as_str())
    }
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
                });
                // Flush the changes.
                self.metadata_handle.flush()?;
            }
        };
        // Create a directory for this experiment in the vault.
        let dir_name = format!("{}/{}", self.base_dir, experiment);
        wrap_io_error(fs::create_dir(path::Path::new(&dir_name)))?;
        Ok(())
    }
}

/// AddExperimentMeta represents a vault add-meta command.
#[derive(Debug)]
struct AddExperimentMeta {
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
        // TODO (rohany): What are we going to do with the metadata though?
        let instance = ExperimentMetaCollectionHandle::new(self.instance.as_str())?;

        // Open up the experiment directory.
        let mut experiment = vault.open_experiment(self.experiment.as_str())?;

        // Copy in the target experiment.
        let id = uuid::Uuid::new_v4();
        experiment.store(self.instance.as_str(), &id.to_string().as_str())?;

        // Write out the instance's metadata.
        let mut instance_meta = experiment.open_instance_meta(&id)?;
        instance_meta.meta = ExperimentInstance {
            name: self.experiment.clone(),
            meta: instance.meta.clone(),
            timestamp: "TODO (rohany): fill this out".to_string(),
            id,
        };
        instance_meta.flush()?;

        // TODO (rohany): Update any metadata in the experiment and the vault, such as the pointer
        //  to the most recent version of the experiment.

        Ok(())
    }
}

/// GetLatestInstance represents a vault get-latest command.
struct GetLatestInstance {
    #[allow(dead_code)]
    experiment: String,
}

impl VaultCommmand for GetLatestInstance {
    fn execute(&self) -> Result<()> {
        // In an ideal world, we should reduce this to a call of Query.
        Err(VaultError::Unimplemented("get-latest".parse().unwrap()))
    }
}

// Meta contains information that is serialized into the base directory of a
// Vault instance.
#[derive(Serialize, Deserialize, Debug)]
pub struct Meta {
    /// categories is a list of registered experiments.
    pub categories: Vec<Experiment>,
    // Is there a higher level version object we can use here?
    pub version: String,
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

// TODO (rohany): Comment this.
struct ExperimentHandle {
    directory: String,
}

impl ExperimentHandle {
    fn new(directory: &str) -> Result<ExperimentHandle> {
        match path::Path::new(directory).is_dir() {
            true => Ok(ExperimentHandle {
                directory: directory.to_string(),
            }),
            false => Err(VaultError::InvalidDirectory(directory.to_string())),
        }
    }

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

    fn open_instance_meta(&mut self, id: &uuid::Uuid) -> Result<ExperimentInstanceHandle> {
        // TODO (rohany): All of these key construction routines should become functions.
        let dir = format!("{}/{}", &self.directory, id.to_string());
        ExperimentInstanceHandle::new(dir.as_str())
    }

    fn iter_instances(&mut self) -> Result<InstanceIterator> {
        let dirs = wrap_io_error(fs::read_dir(&self.directory))?;
        Ok(InstanceIterator { dirs })
    }
}

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
struct FileHandle {
    filename: String,
}

// TODO (rohany): Comment this up.
impl FileHandle {
    fn new(filename: &str) -> FileHandle {
        FileHandle {
            filename: filename.to_string(),
        }
    }

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

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        // Open up the handle's file to read from.
        let fo = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(self.filename.as_str());
        let mut file = wrap_io_error(fo)?;
        let _ = wrap_io_error(file.write(bytes))?;
        Ok(())
    }
}

type ExperimentInstanceHandle = MetadataHandle<ExperimentInstance>;
type ExperimentMetaCollectionHandle = MetadataHandle<ExperimentMetaCollection>;
type VaultMetaHandle = MetadataHandle<VaultMeta>;

// TODO (rohany): Comment this.
#[derive(Debug)]
struct MetadataHandle<T> {
    handle: FileHandle,
    meta: T,
}

impl ExperimentInstanceHandle {
    fn new(dir: &str) -> Result<ExperimentInstanceHandle> {
        // TODO (rohany): Make this a constant.
        MetadataHandle::new_from_dir(dir, ".instancemeta")
    }
}

impl ExperimentMetaCollectionHandle {
    fn new(dir: &str) -> Result<ExperimentMetaCollectionHandle> {
        // TODO (rohany): Make this a constant.
        MetadataHandle::new_from_dir(dir, ".experimentmeta")
    }
}

impl VaultMetaHandle {
    fn new(dir: &str) -> Result<VaultMetaHandle> {
        // TODO (rohany): Make this a constant.
        MetadataHandle::new_from_dir(dir, ".vaultmeta")
    }
}

impl<T> MetadataHandle<T>
where
    T: de::DeserializeOwned + Serialize + Default,
{
    fn new_from_dir(dir: &str, name: &str) -> Result<MetadataHandle<T>> {
        // TODO (rohany): Validate that the input directory actually does exist.
        let path = path::Path::new(dir).join(name);
        let path_str = match path.to_str() {
            Some(s) => s,
            None => panic!("what do we do here?"),
        };
        MetadataHandle::new_from_file(path_str)
    }

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

    fn flush(&mut self) -> Result<()> {
        self.handle
            .write(vault_serde::serialize(&self.meta)?.as_bytes())
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct VaultMeta {
    experiments: Vec<Experiment>,
    // TODO (rohany): Include a version here?
    // TODO (rohany): Include a "modified" timestamp here.
}

impl VaultMeta {
    /// contains_experiment returns whether a given experiment is registered in this meta.
    fn contains_experiment(&self, experiment: &str) -> bool {
        match self
            .experiments
            .iter()
            .find(|e| e.name.as_str() == experiment)
        {
            Some(_) => true,
            None => false,
        }
    }
}

/// wrap_io_error unwraps an io::Result into a vault::Result.
fn wrap_io_error<T>(r: io::Result<T>) -> Result<T> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(VaultError::IOError(e)),
    }
}

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
        cli::Commands::GetLatest { experiment } => Box::new(GetLatestInstance { experiment }),
        cmd => panic!("unhandled command {:?}", cmd),
    };
    command.execute()
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
        let mut experiment = vault.open_experiment("exp").unwrap();
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
