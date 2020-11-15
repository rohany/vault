use crate::cli;
use serde::de;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::{env, fmt, fs, io, path, result};

pub type Result<T> = result::Result<T, VaultError>;

trait VaultCommmand {
    fn execute(&self) -> Result<()>;
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Experiment {
    pub name: String, // Maybe this has a pointer to the latest version of an experiment?
}

#[allow(dead_code)]
struct ExperimentInstance {
    // Name of the experiment this is part of?
    name: String,
    metas: Vec<ExperimentMeta>,
}

// TODO (rohany): Comment this
#[derive(Serialize, Deserialize, Default)]
struct ExperimentMetaCollection {
    metas: Vec<ExperimentMeta>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct ExperimentMeta {
    key: String,
    value: String,
}

#[allow(dead_code)]
struct Entry {
    id: String,
    path: String,
}

struct Vault {
    #[allow(dead_code)]
    base_dir: String,
    metadata_handle: VaultMetaHandle,
}

impl Vault {
    pub fn new() -> Result<Vault> {
        let base_dir = match env::var_os("VAULT_DIR") {
            Some(dir) => dir,
            None => return Err(VaultError::UnsetInstanceDirectory),
        };
        Vault::new_from_dir(base_dir.to_str().unwrap())
    }
    pub fn new_from_dir(instance_dir: &str) -> Result<Vault> {
        Ok(Vault {
            base_dir: instance_dir.to_string(),
            metadata_handle: VaultMetaHandle::new(instance_dir)?,
        })
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
        match vault
            .metadata_handle
            .meta
            .experiments
            .iter()
            .find(|e| e.name == self.experiment.as_str())
        {
            Some(_) => {
                return Err(VaultError::DuplicateExperimentError(
                    self.experiment.clone(),
                ))
            }
            None => {
                // Add the new experiment to the metadata.
                vault.metadata_handle.meta.experiments.push(Experiment {
                    name: self.experiment.to_string(),
                });
                // Flush the changes.
                vault.metadata_handle.flush()?;
            }
        };
        Ok(())
    }
}

/// StoreExperimentInstance represents a vault add command.
struct StoreExperimentInstance {
    #[allow(dead_code)]
    experiment: String,
    #[allow(dead_code)]
    directory: String,
}

impl VaultCommmand for StoreExperimentInstance {
    fn execute(&self) -> Result<()> {
        // Find the .meta file in the vault instance directory and
        //  validate it.
        // Find the .experiment file in the experiment folder and validate it.
        // Make sure that the target experiment exists.
        // Create a new spot for this experiment data and add any metadata
        //  needed for an individual experiment.
        Err(VaultError::Unimplemented("store".parse().unwrap()))
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
#[derive(Debug)]
pub enum VaultError {
    DuplicateExperimentError(String),
    UnsetInstanceDirectory,
    #[allow(dead_code)]
    InvalidInstanceDirectory(String),
    Unimplemented(String),
    IOError(io::Error),
    SerializationError(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::DuplicateExperimentError(s) => write!(f, "experiment {} already exists", s),
            VaultError::Unimplemented(s) => write!(f, "unimplemented command: {:?}", s),
            VaultError::IOError(e) => write!(f, "IO error: {}", e),
            VaultError::SerializationError(s) => write!(f, "Serialization error: {}", s),
            VaultError::InvalidInstanceDirectory(_) => write!(f, "ROHANY WRITE A MESSAGE HERE"),
            VaultError::UnsetInstanceDirectory => {
                write!(f, "please set VAULT_DIR in your environment")
            }
        }
    }
}

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

type ExperimentMetaCollectionHandle = MetadataHandle<ExperimentMetaCollection>;
type VaultMetaHandle = MetadataHandle<VaultMeta>;

// TODO (rohany): Comment this.
struct MetadataHandle<T> {
    handle: FileHandle,
    meta: T,
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
            directory,
        } => Box::new(StoreExperimentInstance {
            experiment,
            directory,
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
    use crate::vault::{
        Experiment, RegisterExperiment, VaultCommmand, VaultError, VaultMetaHandle,
    };

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
        // TODO (rohany): Refactor this to a contains API, rather than inspecting the implementation.
        let handle = VaultMetaHandle::new(path.as_str()).unwrap();
        assert_eq!(
            handle.meta.experiments,
            vec![Experiment {
                name: "exp1".to_string()
            }]
        );

        // Add another experiment, and expect to find it.
        let _ = RegisterExperiment {
            directory: Some(path.clone()),
            experiment: "exp2".to_string(),
        }
        .execute()
        .unwrap();

        let handle = VaultMetaHandle::new(path.as_str()).unwrap();
        assert_eq!(
            handle.meta.experiments,
            vec![
                Experiment {
                    name: "exp1".to_string()
                },
                Experiment {
                    name: "exp2".to_string()
                }
            ]
        );
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
