/*

// TODO (rohany): This comment is no longer true. It needs to be updated.
An overall idea of how to structure this thing.

There will be a VAULT_DIR that can be configured (or placed in the environment).
This will be where all of the experiment data is stored. On Sherlock for example,
this would need to be stored in the scratch filesystem.

The organization could be as follows:

VAULT_DIR/
|_ .vault - General metadata about the Vault binary, experiments etc.
|_ experiments
  |_ <experiment UUID>
    |_ .experimentmeta - Holds metadata about the experiment, including user defined stuff.
    |_ <user ingested folders...> - Maybe this is given a new name. We could just move the user
                                    experiment into a folder titled with the timestamp of creation.

Experiment directories:
root
|_ .vault - vault commands run by the experiment will be stored in a serialized format here.
|_ stuff generated by the experiment

 */

#[macro_use]
extern crate diesel;
mod cli;
mod vault;
use structopt::StructOpt;

fn main() {
    let args = cli::Commands::from_args();
    match vault::dispatch_command_line(args) {
        Ok(f) => println!("{}", f.to_string()),
        Err(e) => println!("Error: {}", e),
    }
}
