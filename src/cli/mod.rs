use std::str::FromStr;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
/// Vault is a tool for managing and organizing experimental data.
///
/// A standard Vault workflow will create an experiment using `vault register`,
/// generate instances of the experiment with metadata using `vault add-meta`,
/// and then store the data in Vault using `vault store`.
//
// Commands is an enum representing the possible command line invocations.
// Currently, vault supports the following commands:
//   * vault register <experiment_name>
//   * vault store <experiment_name> <instance_directory>
//   * vault add-meta <instance_directory> [kv <key> <value> | json <json dict>]
//   * vault list-meta <instance_directory>
//   * vault get-latest <experiment_name>
//   * vault query <experiment_name> <query>
//
// All commands here must be included in integration tests in tests/cli.rs.
// Additionally, all commands must "doc" comments (///), as these are included
// in the generated help messages visible to end users.
pub enum Commands {
    /// Register a new experiment.
    ///
    /// Instances of the created experiment can be added using `vault store`.
    Register {
        /// Name of the experiment to register.
        experiment: String,
    },
    /// Store an instance of an experiment in Vault.
    Store {
        /// Name of the target experiment.
        experiment: String,
        /// Path to an experiment instance directory.
        instance: String,
    },
    // TODO (rohany): This could be a bigger "meta" command that people can use
    // to adjust the meta on their experiment.
    /// Add metadata to an experiment instance.
    ///
    /// add-meta can only be used to attach metadata to an experiment instance before it
    /// has been added to Vault.
    AddMeta {
        /// Path to an experiment instance directory.
        directory: String,
        #[structopt(subcommand)]
        /// The kind of metadata to add.
        meta: AddMetaKind,
    },
    /// List metadata attached to an experiment instance.
    ListMeta {
        /// Path to an experiment instance directory.
        directory: String,
    },
    /// Get the latest instance of an experiment.
    ///
    /// get-latest returns a directory containing the instance.
    GetLatest {
        /// Name of the target experiment.
        experiment: String,
    },
    /// Query an experiment for instances satisfying some criteria.
    Query {
        /// Name of the target experiment.
        experiment: String,
        /// Query in the Vault Query DSL (detailed in [TODO (rohany)]).
        query: String,
    },
    Weija {
        #[structopt(
            long = "--kind",
            default_value = "instance-dir",
            possible_values = &["instance-dir", "instance-id"],
            case_insensitive = true
        )]
        kind: InstanceResultKind,
    },
}

#[derive(StructOpt, Debug)]
// AddMetaKind represents a kind of metadata to add to an instance. This data
// can either be a single key-value pair, or a JSON set of key-value pairs.
/// The kind of metadata to attach -- either a key-value pair, or a JSON dictionary.
pub enum AddMetaKind {
    /// Add a key-value pair.
    KV { key: String, value: String },
    /// Add all of the key-value pairs in a JSON dictionary.
    JSON { json: String },
}

#[derive(Debug)]
pub enum InstanceResultKind {
    InstanceDirectory,
    InstanceID,
}

impl FromStr for InstanceResultKind {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let as_lower = s.to_lowercase();
        match as_lower.as_str() {
            "instance-dir" => Ok(InstanceResultKind::InstanceDirectory),
            "instance-id" => Ok(InstanceResultKind::InstanceID),
            _ => Err("Unknown instance kind"),
        }
    }
}
