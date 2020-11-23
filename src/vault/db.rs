use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use std::path;

/// DB_FILE is the filename of the SQLite database in each Vault instance.
const DB_FILE: &str = ".vaultdb";

// TODO (rohany): We might want to refactor this to return a different struct?
//  This could hold onto the connection as well as hold some prepared statements.
pub fn open(base_dir: &str) -> SqliteConnection {
    // See if the database file exists already.
    let path = path::Path::new(base_dir).join(DB_FILE);
    // TODO (rohany): Don't unwrap this.
    let path_str = path.to_str().unwrap();
    let exists = path.exists();
    let conn = SqliteConnection::establish(path_str).unwrap();

    // If database didn't exist already, then initialize the database with
    // all of the tables that need to be present.
    if !exists {
        // TODO (rohany): Don't panic on failure here.
        conn.batch_execute(statements::INITIALIZE).unwrap();
    };
    // TODO (rohany): Don't panic here.
    // Execute any queries that need to be run on connection startup.
    conn.batch_execute(statements::ON_STARTUP).unwrap();
    conn
}

/// statements is a module containing predefined SQL statements for use by Vault.
mod statements {
    // TODO (rohany): Think about how migrations are going to work later.

    /// INITIALIZE is the SQL statement to initialize a SQLite instance for vault.
    pub const INITIALIZE: &'static str = "
CREATE TABLE experiment (
    id INTEGER PRIMARY KEY,
    name TEXT
);
CREATE TABLE experiment_instance (
    id INTEGER PRIMARY KEY,
    experiment_id INTEGER, -- Foreign key relationship to the experiment table.
    -- A timestamp for when the instance was created. We can't use this to
    -- answer the get-latest query, because the timestamp's aren't at a high
    -- enough resolution.
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    -- A vault created UUID for the row. We need this as SQLite doesn't support 
    -- RETURNING, so we can't get the SQLite created ID for a instance when we
    -- create it.
    uuid TEXT,
    -- The user-supplied JSON metadata for the instance. It is stored as TEXT 
    -- because SQLite doesn't have a JSON column.
    meta_text TEXT, 
    FOREIGN KEY (experiment_id) REFERENCES experiment (id)
);
";
    // TODO (rohany): Add some indexes here. In particular, an expression index
    //  on the JSON meta column.

    /// ON_STARTUP is a set of statements that need to be executed when a
    /// connection to the SQLite instance is made.
    pub const ON_STARTUP: &'static str = "
PRAGMA foreign_keys = ON;
";
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
