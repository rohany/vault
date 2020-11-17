use structopt::StructOpt;

#[derive(StructOpt, Debug)]
/// Commands is an enum representing the possible command line invocations.
/// Currently, vault supports the following commands:
///   * vault register <experiment_name>
///   * vault store <experiment_name> <instance_directory>
///   * vault add-meta <instance_directory> <key> <value>
///   * vault get-latest <experiment_mame>
///   * vault query ...
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
        key: String,
        value: String,
    },
    GetLatest {
        experiment: String,
    },
    Query,
}
