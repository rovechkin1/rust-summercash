use jsonrpc_core::{response::Output, Error, ErrorCode, IoHandler, Result};
use jsonrpc_derive::rpc;

use walkdir::WalkDir;

use serde::Deserialize;

use super::{
    super::super::{
        accounts::account::Account,
        common::address::Address,
        core::{
            sys::{
                proposal::{Operation, Proposal, ProposalData},
                system::System,
            },
            types::{
                graph::Node,
                signature::Signature,
                state::Entry,
                transaction::{self, Transaction},
            },
        },
        crypto::hash::Hash,
    },
    error,
};

use num::BigUint;

use std::{
    collections::HashMap,
    sync::{Arc, RwLock, RwLockWriteGuard},
};

/// Defines the standard SummerCash DAG RPC API.
#[rpc]
pub trait Dag {
    /// Gets a list of nodes contained in the currently attached network's DAG.
    #[rpc(name = "get_dag")]
    fn get(&self) -> Result<Vec<Node>>;

    /// Gets a list of transaction hashes stored in the currently attached DAG.
    #[rpc(name = "list_transactions")]
    fn list(&self) -> Result<Vec<Hash>>;

    /// Creates a new transaction with the provided sender, recipient, value, and payload.
    #[rpc(name = "create_transaction")]
    fn create_tx(
        &self,
        sender: String,
        recipient: String,
        value: String,
        payload: String,
    ) -> Result<Transaction>;

    /// Signs the transaction with the provided hash.
    #[rpc(name = "sign_transaction")]
    fn sign_tx(&self, hash: String, data_dir: String) -> Result<Signature>;

    /// Gets a list of transactions contained in the transaction cache.
    #[rpc(name = "get_mem_transactions")]
    fn get_mem_txs(&self, data_dir: String) -> Result<Vec<Hash>>;

    /// Signs a transaction with the provided hash in the provided data directory.
    #[rpc(name = "publish_transaction")]
    fn publish_tx(&self, hash: String, data_dir: String) -> Result<()>;
}

/// An implementation of the DAG API.
pub struct DagImpl {
    pub(crate) runtime: Arc<RwLock<System>>,
}

impl Dag for DagImpl {
    /// Gets a list of nodes contained in the currently attached network's DAG.
    fn get(&self) -> Result<Vec<Node>> {
        if let Ok(rt) = self.runtime.read() {
            // The finalized set of nodes contained in the DAG
            let mut collected_nodes: Vec<Node> = Vec::new();

            debug!("Collecting {} nodes in the DAG", rt.ledger.nodes.len());

            // Iterate through each of the nodes in the graph, and purely obtain the full representation of such nodes
            for i in 0..rt.ledger.nodes.len() {
                match rt.ledger.get_pure(i) {
                    Ok(Some(node)) => collected_nodes.push(node.clone()),
                    Ok(None) | Err(_) => {
                        debug!("Skipping node {}", i);
                        continue;
                    }
                };
            }

            // Return all of the nodes in the runtime's ledger
            Ok(collected_nodes)
        } else {
            debug!("Unable to obtain a lock on the client's runtime");

            // Return the corresponding error
            Err(Error::new(ErrorCode::from(
                error::ERROR_UNABLE_TO_OBTAIN_LOCK,
            )))
        }
    }

    /// Gets a list of transaction hashes stored in the currently attached DAG.
    fn list(&self) -> Result<Vec<Hash>> {
        if let Ok(rt) = self.runtime.read() {
            // Return all of the keys, which are the node hashes, stored in the DAG
            Ok(rt.ledger.hash_routes.keys().copied().collect())
        } else {
            debug!("Unable to obtain a lock on the client's runtime");

            // Return the corresponding error
            Err(Error::new(ErrorCode::from(
                error::ERROR_UNABLE_TO_OBTAIN_LOCK,
            )))
        }
    }

    /// Creates a new transaction with the provided sender, recipient, value, and payload.
    fn create_tx(
        &self,
        sender: String,
        recipient: String,
        value: String,
        payload: String,
    ) -> Result<Transaction> {
        // Convert the provided sender and recipient values to addresses
        let sender_address = Address::from(sender);
        let recipient_address = Address::from(recipient);

        // Get a lock on the client's runtime
        let runtime = if let Ok(rt) = self.runtime.read() {
            rt
        } else {
            debug!("Unable to obtain a lock on the client's runtime");

            // Return a mutex error
            return Err(Error::new(ErrorCode::from(
                error::ERROR_UNABLE_TO_OBTAIN_LOCK,
            )));
        };

        // Get a head from the DAG. This is necessary, as we need to determine what nonce we can use for the tx.
        let (head, head_entry): (Node, Entry) =
            if let Some(mut h) = runtime.ledger.obtain_executed_head() {
                // Load the entry's state data
                if let Some(state_entry) = h.state_entry.take() {
                    (h, state_entry)
                } else {
                    debug!("Best graph node doesn't contain a state entry; terminating");

                    // Return a state ref error
                    return Err(Error::new(ErrorCode::from(
                        error::ERROR_UNABLE_TO_OBTAIN_STATE_REF,
                    )));
                }
            } else {
                debug!("Unable to obtain a lock on the client's runtime");

                // Return a state ref error
                return Err(Error::new(ErrorCode::from(
                    error::ERROR_UNABLE_TO_OBTAIN_STATE_REF,
                )));
            };

        // Get a list of children associated with the last cleared node.
        let head_children_opt = runtime.ledger.node_children.get(&head.hash);

        // The parents of the transaction we're about to generate
        let mut parent_hashes: Vec<Hash> = Vec::new();

        // Only collect the head children if they actually exist
        if let Some(head_children) = head_children_opt {
            // We're going to try to resolve each of the children associated with the last cleared transaction
            for child in head_children {
                // Only use the child as a parent of the new transaction if it unresolved.
                if runtime.ledger.hash_routes.contains_key(child)
                    && runtime.ledger.nodes[*runtime.ledger.hash_routes.get(child).unwrap()]
                        .state_entry
                        .is_none()
                {
                    // Add the child as a parent of the new transaction
                    parent_hashes.push(*child);
                }
            }
        }

        // The index of the transaction in the set of user transactions
        let mut nonce = 0;

        if let Some(last_nonce) = head_entry.data.nonces.get(&sender_address.to_str()) {
            nonce = last_nonce + 1;
        }

        // Create a new transaction using the last defined nonce in the global state
        let mut transaction = Transaction::new(
            nonce,
            sender_address,
            recipient_address,
            BigUint::from_bytes_be(&value.into_bytes()),
            payload.as_bytes(),
            parent_hashes,
        );

        // Calculate a merged state entry for each of the parents of the transaction. We can use this to provide a proof of correctness for this tx.
        let (merged_state_entry, parent_entries) = if let Ok(res) = runtime
            .ledger
            .resolve_parent_nodes(transaction.transaction_data.parents.clone())
        {
            res
        } else {
            debug!(
                "Failed to merge the parent entries required to produce transaction {}",
                transaction.hash
            );

            // Return a state error
            return Err(Error::new(ErrorCode::from(
                error::ERROR_UNABLE_TO_OBTAIN_STATE_REF,
            )));
        };

        // Register the parent states
        transaction.register_parental_state(merged_state_entry, parent_entries);

        // Return the transaction
        Ok(transaction)
    }

    /// Signs the transaction with the provided hash.
    fn sign_tx(&self, hash: String, data_dir: String) -> Result<Signature> {
        // Read the transaction from the disk
        let mut tx: Transaction =
            if let Ok(tx) = Transaction::from_disk_at_data_directory(&data_dir, Hash::from(hash)) {
                tx
            } else {
                // Return an error representing the inabiility of the tx to be opened
                return Err(Error::new(ErrorCode::from(
                    error::ERROR_UNABLE_TO_OPEN_TRANSACTION,
                )));
            };

        // Read the account from the disk
        let acc = if let Ok(a) =
            Account::read_from_disk_at_data_directory(tx.transaction_data.sender, &data_dir)
        {
            a
        } else {
            // Return an error
            return Err(Error::new(ErrorCode::from(
                error::ERROR_UNABLE_TO_OPEN_ACCOUNT,
            )));
        };

        // Try to get a keypair for the account that we've opened
        let keypair = if let Ok(k) = acc.keypair() {
            k
        } else {
            // Return an error
            return Err(Error::new(ErrorCode::from(
                error::ERROR_SIGNATURE_UNDEFINED,
            )));
        };

        // Sign the transaction, and return it
        match transaction::sign_transaction(keypair, &mut tx) {
            Ok(_) => Ok(if let Some(sig) = tx.signature.clone() {
                // Persist the tx to the disk, now that it's been signed
                match tx.to_disk_at_data_directory(&data_dir) {
                    Ok(_) => sig,
                    Err(_) => {
                        // Return an I/O error
                        return Err(Error::new(ErrorCode::from(
                            error::ERROR_UNABLE_TO_WRITE_TRANSACTION,
                        )));
                    }
                }
            } else {
                return Err(Error::new(ErrorCode::from(
                    error::ERROR_SIGNATURE_UNDEFINED,
                )));
            }),
            Err(_) => Err(Error::new(ErrorCode::from(
                error::ERROR_SIGNATURE_UNDEFINED,
            ))),
        }
    }

    /// Gets a list of transactions contained in the transaction cache.
    fn get_mem_txs(&self, data_dir: String) -> Result<Vec<Hash>> {
        // Make an instance of a directory walker so that we can collect a list of memory-bound transactions
        let wk = WalkDir::new(format!("{}/mem", data_dir));

        // Make an empty list that we can store each of the hashes
        let mut transactions: Vec<Hash> = Vec::new();

        // Go through each of the transaction files
        for file in wk.into_iter().filter_map(|e| e.ok()) {
            // Get the name of the file so that we can determine its hash
            if let Ok(meta) = file.metadata() {
                // Only use the file's info if it isn't a directory
                if !meta.is_file() {
                } else {
                    continue;
                }
            } else {
                continue;
            }

            // Try to derive a hash from the file's name
            if let Some(f_name) = file.path().to_str() {
                if let Some(tx_hash) = f_name.split(".json").collect::<Vec<&str>>().get(0) {
                    // Add the transaction hash to the list of hashes
                    transactions.push(Hash::from(*tx_hash));
                }
            }
        }

        // Return the list of tx hashes
        Ok(transactions)
    }

    /// Signs the transaction with the provided hash in the given data directory.
    fn publish_tx(&self, hash: String, data_dir: String) -> Result<()> {
        // Open the transaction so that we can use it to publish a proposal derived from it on the network
        let tx: Transaction =
            if let Ok(t) = Transaction::from_disk_at_data_directory(&data_dir, Hash::from(hash)) {
                t
            } else {
                // Return an error reflecting the inability of the executor to generate this proposal
                return Err(Error::new(ErrorCode::from(
                    error::ERROR_UNABLE_TO_OPEN_TRANSACTION,
                )));
            };

        // Make a proposal to submit the provided transaction
        let proposal_data = ProposalData::new(
            "ledger::transactions".to_owned(),
            Operation::Append {
                value_to_append: tx.to_bytes(),
            },
        );

        // Make a proposal for the transaction
        let proposal = Proposal::new(format!("new_tx({})", tx.hash.to_str()), proposal_data);

        // Try to get a lock on the server's runtime
        let mut rt: RwLockWriteGuard<System> = if let Ok(rt) = self.runtime.write() {
            rt
        } else {
            return Err(Error::new(ErrorCode::from(
                error::ERROR_UNABLE_TO_OBTAIN_LOCK,
            )));
        };

        // Register the proposal
        rt.register_proposal(proposal);

        Ok(())
    }
}

impl DagImpl {
    /// Registers the DAG service on the given IoHandler server.
    pub fn register(io: &mut IoHandler, runtime: Arc<RwLock<System>>) {
        // Register this service on the IO handler
        io.extend_with(Self { runtime }.to_delegate());
    }
}

/// A client for the SummerCash DAG API.
pub struct Client {
    /// The address for the server hosting the APi
    pub server: String,

    /// An HTTP client
    client: reqwest::Client,
}

impl Client {
    /// Initializes a new Client with the given remote URL.
    pub fn new(server_addr: &str) -> Self {
        // Initialize and return the client
        Self {
            server: server_addr.trim_end_matches('/').to_owned(),
            client: reqwest::Client::new(),
        }
    }

    /// Performs a request considering the given method, and returns the response.
    async fn do_request<T>(
        &self,
        method: &str,
        params: &str,
    ) -> std::result::Result<T, failure::Error>
    where
        T: for<'de> Deserialize<'de>,
    {
        // Make a hashmap to store the body of the request in
        let mut json_body: HashMap<&str, serde_json::Value> = HashMap::new();
        json_body.insert("jsonrpc", serde_json::Value::String("2.0".to_owned()));
        json_body.insert("method", serde_json::Value::String(method.to_owned()));
        json_body.insert("id", serde_json::Value::String("".to_owned()));
        json_body.insert("params", serde_json::from_str(params)?);

        // Send a request to the endpoint, and pass the given parameters along with the request
        let res = self
            .client
            .post(&self.server)
            .json(&json_body)
            .send()
            .await?
            .json::<Output>()
            .await?;

        // Some type conversion black magic fuckery
        match res {
            Output::Success(s) => match serde_json::from_value(s.result) {
                Ok(res) => Ok(res),
                Err(e) => Err(e.into()),
            },
            Output::Failure(e) => Err(e.error.into()),
        }
    }

    /// Gets a list of nodes in the working graph.
    pub async fn get(&self) -> std::result::Result<Vec<Node>, failure::Error> {
        self.do_request::<Vec<Node>>("get_dag", "[]").await
    }

    /// Gets a list of transaction hashes contained in the working DAG.
    pub async fn list(&self) -> std::result::Result<Vec<Hash>, failure::Error> {
        self.do_request::<Vec<Hash>>("list_transactions", "[]")
            .await
    }

    /// Creates a new transaction with the provided parameters.
    pub async fn create_tx(
        &self,
        sender: String,
        recipient: String,
        amount: u128,
        payload: String,
    ) -> std::result::Result<Transaction, failure::Error> {
        self.do_request::<Transaction>(
            "create_transaction",
            &format!(
                r#"[{}, {}, "{}", {}]"#,
                serde_json::to_string(&sender)?,
                serde_json::to_string(&recipient)?,
                serde_json::to_string(&amount)?,
                serde_json::to_string(&payload)?
            ),
        )
        .await
    }

    /// Signs the transaction with the provided account.
    pub async fn sign_tx(
        &self,
        hash: String,
        data_dir: String,
    ) -> std::result::Result<Signature, failure::Error> {
        self.do_request::<Signature>(
            "sign_transaction",
            &format!(
                "[{}, {}]",
                &serde_json::to_string(&hash)?,
                &serde_json::to_string(&data_dir)?
            ),
        )
        .await
    }

    /// Gets a list of pending transactions stored on the disk.
    pub async fn get_mem_txs(
        &self,
        data_dir: String,
    ) -> std::result::Result<Vec<Hash>, failure::Error> {
        self.do_request::<Vec<Hash>>(
            "get_mem_transactions",
            &format!("[{}]", &serde_json::to_string(&data_dir)?),
        )
        .await
    }

    /// Publishes a transaction stored on the disk.
    pub async fn publish_tx(
        &self,
        hash: String,
        data_dir: String,
    ) -> std::result::Result<(), failure::Error> {
        self.do_request::<()>(
            "publish_transaction",
            &format!(
                "[{}, {}]",
                &serde_json::to_string(&hash)?,
                serde_json::to_string(&data_dir)?
            ),
        )
        .await
    }
}
