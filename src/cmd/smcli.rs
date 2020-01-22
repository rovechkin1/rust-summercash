/// SMCli is the SummerCash command-line interface.
extern crate clap;
#[macro_use]
extern crate log;
use clap::Clap;

use summercash::{common::address::Address, crypto::hash::Hash, p2p::rpc::accounts};

use std::clone::Clone;

/// The SummerCash command-line interface.
#[derive(Clap)]
#[clap(version = "1.0", author = "Dowland A.")]
struct Opts {
    /// Print debug info
    #[clap(short = "d", long = "debug")]
    debug: bool,

    /// Prevents any non-critical information from being printed to the console
    #[clap(short = "s", long = "silent")]
    silent: bool,

    /// Changes the directory that node data will be stored in
    #[clap(long = "data-dir", default_value = "data")]
    data_dir: String,

    /// Signals to the SummerCash command-line utility that it should connect to the given node.
    #[clap(
        short = "r",
        long = "remote-host-url",
        default_value = "http://127.0.0.1:8080"
    )]
    rpc_host_url: String,

    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap, Clone)]
enum SubCommand {
    /// Creates a new SummerCash object of a given type.
    #[clap(name = "create")]
    Create(Create),

    /// Gets a SummerCash object of a given type.
    #[clap(name = "get")]
    Get(Get),

    /// Locks a SummerCash object of a given type.
    #[clap(name = "lock")]
    Lock(Lock),

    /// Unlocks a SummerCash object of a given type.
    #[clap(name = "unlock")]
    Unlock(Unlock),

    /// Deletes a SummerCash object of a given type.
    #[clap(name = "delete")]
    Delete(Delete),

    /// Gets a list of SummerCash objects of a given type.
    #[clap(name = "list")]
    List(List),
}

#[derive(Clap, Clone)]
enum Create {
    /// Creates a new account.
    Account,
}

#[derive(Clap, Clone)]
enum Get {
    /// Gets a particular account with the given address.
    Account(Account),
}

#[derive(Clap, Clone)]
enum Lock {
    /// Locks a particular account with the given address.
    Account(CryptoAccount),
}

#[derive(Clap, Clone)]
enum Unlock {
    /// Unlocks a particular account with the given address.
    Account(CryptoAccount),
}

#[derive(Clap, Clone)]
enum Delete {
    /// Deletes an account with the given address.
    Account(Account),
}

#[derive(Clap, Clone)]
enum List {
    /// Gets a list of accounts stored on the disk.
    Accounts(UnitAccount),
}

#[derive(Clap, Clone)]
struct Account {
    /// The address of the account
    address: String,
}

#[derive(Clap, Clone)]
struct CryptoAccount {
    /// The address of the account
    address: String,

    /// The encryption / decryption key used to unlock or lock the account
    key: String,
}

#[derive(Clap, Clone)]
struct UnitAccount {}

#[tokio::main]
async fn main() -> Result<(), failure::Error> {
    // Get the options that the user passed to the program
    let opts: Opts = use_options(Opts::parse())?;

    match opts.subcmd.clone() {
        SubCommand::Create(c) => create(opts, c).await,
        SubCommand::Get(c) => get(opts, c).await,
        SubCommand::Lock(l) => lock(opts, l).await,
        SubCommand::Unlock(u) => unlock(opts, u).await,
        SubCommand::Delete(d) => delete(opts, d).await,
        SubCommand::List(l) => list(opts, l).await,
    }
}

/// Creates the object from the given options.
async fn create(opts: Opts, c: Create) -> Result<(), failure::Error> {
    match c {
        Create::Account => {
            // Make a client for the accounts API
            let client = accounts::Client::new(&opts.rpc_host_url);

            // Generate the account
            match client.generate(&opts.data_dir).await {
                Ok(acc) => info!("Successfully generated account: {}", acc),
                Err(e) => error!("Failed to generate account: {}", e),
            }
        }
    };

    Ok(())
}

/// Gets the object with matching criteria.
async fn get(opts: Opts, g: Get) -> Result<(), failure::Error> {
    match g {
        Get::Account(acc) => {
            // Make a client for the accounts API
            let client = accounts::Client::new(&opts.rpc_host_url);

            // Get the account
            match client
                .get(Address::from(Hash::from_str(&acc.address)?), &opts.data_dir)
                .await
            {
                Ok(acc) => info!("Found account: {}", acc),
                Err(e) => error!("Failed to load the account: {}", e),
            }
        }
    };

    Ok(())
}

/// Locks the object with matching constraints.
async fn lock(opts: Opts, l: Lock) -> Result<(), failure::Error> {
    match l {
        Lock::Account(acc) => {
            // Make a client for the accounts API
            let client = accounts::Client::new(&opts.rpc_host_url);

            // Lock the account
            match client
                .lock(
                    Address::from(Hash::from_str(&acc.address)?),
                    &acc.key,
                    &opts.data_dir,
                )
                .await
            {
                Ok(_) => info!("Locked account '{}' successfully", acc.address),
                Err(e) => error!("Failed to lock the account: {}", e),
            }
        }
    };

    Ok(())
}

/// Locks the object with matching constraints.
async fn unlock(opts: Opts, u: Unlock) -> Result<(), failure::Error> {
    match u {
        Unlock::Account(acc) => {
            // Make a client for the accounts API
            let client = accounts::Client::new(&opts.rpc_host_url);

            // Lock the account
            match client
                .unlock(
                    Address::from(Hash::from_str(&acc.address)?),
                    &acc.key,
                    &opts.data_dir,
                )
                .await
            {
                Ok(acc) => info!("Unlocked account successfully: {}", acc),
                Err(e) => error!("Failed to lock the account: {}", e),
            }
        }
    };

    Ok(())
}

/// Deletes the object with matching constraints.
async fn delete(opts: Opts, d: Delete) -> Result<(), failure::Error> {
    match d {
        Delete::Account(acc) => {
            // Make a client for the accounts API
            let client = accounts::Client::new(&opts.rpc_host_url);

            // Delete the account
            match client
                .delete(Address::from(Hash::from_str(&acc.address)?), &opts.data_dir)
                .await
            {
                Ok(_) => info!("Deleted account '{}' successfully", acc.address),
                Err(e) => error!("Failed to delete account '{}': {}", acc.address, e),
            }
        }
    };

    Ok(())
}

/// Lists the objects with the given type.
async fn list(opts: Opts, l: List) -> Result<(), failure::Error> {
    match l {
        List::Accounts(_) => {
            // Make a client for the accounts API
            let client = accounts::Client::new(&opts.rpc_host_url);

            // List all of the accounts on the disk
            match client.list(&opts.data_dir).await {
                // Print out each of the accounts' addresses
                Ok(accounts) => {
                    // The collective addresses of each of the accounts, in one string
                    let mut accounts_string = String::new();

                    // The current index in the addr collection process
                    let mut i = 0;

                    // Put each of the addresses into the overall string
                    let _: Vec<()> = accounts
                        .iter()
                        .map(|addr| {
                            // Append the address to the overall string (+ a separator, if need be)
                            accounts_string +=
                                &format!("{}{}", if i > 0 { ", " } else { "" }, addr.to_str());

                            // Increment the current index
                            i += 1;

                            ()
                        })
                        .collect();

                    info!("Found accounts: {}", accounts_string);
                }

                // Log the error
                Err(e) => error!("Failed to locate all of the accounts in dir: {}", e),
            }
        }
    }

    Ok(())
}

/// Applies the given options.
fn use_options(mut opts: Opts) -> Result<Opts, failure::Error> {
    // Configure the logger
    if !opts.silent {
        if opts.debug {
            // Include debug statements in the logger output
            env_logger::builder()
                .filter_level(log::LevelFilter::Debug)
                .init();
        } else {
            // Include just up to info statements
            env_logger::builder()
                .filter_level(log::LevelFilter::Info)
                .init();
        }
    }

    // If the user has chosen the default data dir, normalize it
    if opts.data_dir == "data" {
        // Normalize the data directory, and put it back in the config
        opts.data_dir = summercash::common::io::data_dir();
    }

    Ok(opts)
}