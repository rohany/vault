use structopt::StructOpt;

// Maybe put this into it's own module?
#[derive(StructOpt, Debug)]
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
