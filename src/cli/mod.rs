use structopt::StructOpt;

#[derive(StructOpt, Debug)]
/// Commands is an enum representing the possible command line invocations.
/// Currently, vault supports the following commands:
///   * vault register <experiment_name>
///   * vault store <experiment_name> <instance_directory>
///   * vault add-meta <instance_directory> <key> <value>
///   * vault list-meta <instance_directory>
///   * vault get-latest <experiment_name>
///   * vault query <experiment_name> <query>
pub enum Commands {
    Register {
        experiment: String,
    },
    Store {
        experiment: String,
        instance: String,
    },
    // This could be a bigger "meta" command that people can use to adjust the meta
    // on their experiment.
    AddMeta {
        directory: String,
        #[structopt(subcommand)]
        meta: AddMetaKind,
    },
    ListMeta {
        directory: String,
    },
    GetLatest {
        experiment: String,
    },
    Query {
        experiment: String,
        query: String,
    },
}

#[derive(StructOpt, Debug)]
// AddMetaKind represents a kind of metadata to add to an instance. This data
// can either be a single key-value pair, or a JSON set of key-value pairs.
pub enum AddMetaKind {
    KV { key: String, value: String },
    JSON { json: String },
}
