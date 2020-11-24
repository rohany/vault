use diesel;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use std::path;

// embed_migrations embeds the migrations defined at vault/migrations/ into the binary.
embed_migrations!();

/// DB_FILE is the filename of the SQLite database in each Vault instance.
const DB_FILE: &str = ".vaultdb";

// TODO (rohany): We might want to refactor this to return a different struct?
//  This could hold onto the connection as well as hold some prepared statements.
// TODO (rohany): Remove all of the .unwrap()'s in this method.
pub fn open(base_dir: &str) -> SqliteConnection {
    // See if the database file exists already.
    let path = path::Path::new(base_dir).join(DB_FILE);
    let path_str = path.to_str().unwrap();
    let conn = SqliteConnection::establish(path_str).unwrap();
    // Perform any needed migrations.
    embedded_migrations::run(&conn).unwrap();
    // Execute any queries that need to be run on connection startup.
    conn.batch_execute(statements::ON_STARTUP).unwrap();
    conn
}

/// statements is a module containing predefined SQL statements for use by Vault.
mod statements {
    // TODO (rohany): Add some indexes here. In particular, an expression index
    //  on the JSON meta column.

    /// ON_STARTUP is a set of statements that need to be executed when a
    /// connection to the SQLite instance is made.
    pub const ON_STARTUP: &'static str = "PRAGMA foreign_keys = ON;";
}

/// schema contains the actual tables and models to interact with the database.
pub mod schema {
    use chrono;
    use diesel::{Insertable, Queryable};

    // The experiment table. Contains an ID and the name of the experiment.
    table! {
        experiment {
            id -> Integer,
            name -> Text,
        }
    }

    #[derive(Debug, Queryable)]
    /// Experiment is a Queryable view onto the experiment table.
    pub struct Experiment {
        pub id: i32,
        pub name: String,
    }
    #[derive(Insertable)]
    #[table_name = "experiment"]
    /// ExperimentInsert the structure used to insert values into the
    /// experiment table.
    pub struct ExperimentInsert<'a> {
        pub name: &'a str,
    }

    // The experiment_instance table contains metadata about experiment instances.
    // Each row represents an instance, and has a pointer to the experiment that
    // it is an instance of.
    table! {
        experiment_instance {
            id -> Integer,
            experiment_id -> Integer,
            created_at -> Timestamp,
            uuid -> Text,
            meta_text -> Text,
        }
    }
    // The experiment_instance table is joinable to the experiment table.
    joinable!(experiment_instance -> experiment(id));
    // Some diesel magic that we have to do to actually do joins between the tables.
    allow_tables_to_appear_in_same_query!(experiment, experiment_instance);

    #[derive(Debug, Queryable)]
    /// ExperimentInstance is the queryable view onto the experiment_instance table.
    pub struct ExperimentInstance {
        /// _id should not be used by consumers of ExperimentInstance. Uses of _id
        /// should most likely use uuid instead.
        pub _id: i32,
        pub experiment_id: i32,
        // As mentioned above, the created_at timestamp column is not accurate
        // enough to use for the get-latest queries.
        pub created_at: chrono::NaiveDateTime,
        pub uuid: String,
        pub meta_text: String,
    }

    #[derive(Insertable)]
    #[table_name = "experiment_instance"]
    /// ExperimentInstanceInsert is the structure used to insert values into the
    /// experiment_instance table.
    pub struct ExperimentInstanceInsert<'a> {
        pub experiment_id: i32,
        pub uuid: &'a str,
        pub meta_text: &'a str,
    }
}
