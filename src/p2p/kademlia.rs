use super::{
    super::{
        core::{
            sys::proposal::{Operation, Proposal, ProposalData},
            types::transaction::Transaction,
        },
        crypto::hash::Hash,
    },
    client::ClientBehavior,
    sync,
};

use libp2p::{
    kad::{
        record::{Key, Record},
        KademliaEvent, Quorum, QueryResult,
    },
    swarm::NetworkBehaviourEventProcess,
};
use libp2p::kad::PeerRecord;


/// Network synchronization via KAD DHT events.
/// Synchronization of network proposals, for example, is done in this manner.
impl NetworkBehaviourEventProcess<KademliaEvent> for ClientBehavior {
    // Wait for a peer to send us a kademlia event message. Once this happens, we can try to use the message for something (e.g. synchronization).

    // fn inject_event(&mut self, event: KademliaEvent) {
    //     match event {
    //         KademliaEvent::QueryResult { id, result, .. } => match result {
    //             QueryResult::GetRecord(result) => {}
    //             _ => {}
    //         }
    //         _ => {}
    //     }
    // }
    fn inject_event(&mut self, event: KademliaEvent) {
        match event {
            KademliaEvent::QueryResult { id, result, .. } => match result {
                // The record was found successfully; print it
                QueryResult::GetRecord(Ok(result)) => {
                    for PeerRecord { peer, record, .. } in result.records {
                        // Handle different key types
                        match record.key.as_ref() {
                            b"ledger::transactions::root" => {
                                // Convert the pure bytes into a hash primitive
                                let root_hash = Hash::new(record.value);

                                // Alert the user that we've determined what the hash of the root tx is
                                info!(
                                    "Received the root transaction hash for the network: {}",
                                    root_hash
                                );
                                let q: Quorum = self.active_subset_quorum();

                                // Get the actual root transaction, not just the hash, from the network
                                self.kad_dht.get_record(
                                    &Key::new(&sync::transaction_with_hash_key(root_hash)),
                                    q,
                                );
                            }

                            _ => {
                                // If the response is a transaction response, try deserializing the transaction, and doing something with it
                                if String::from_utf8_lossy(record.key.as_ref())
                                    .contains("ledger::transactions::tx")
                                {
                                    // Deserialize the transaction that the peer responded with
                                    let tx: Transaction =
                                        if let Ok(val) = bincode::deserialize::<Transaction>(&record.value) {
                                            // Alert the user that we've obtained a copy of the tx
                                            info!(
                                                "Obtained a copy of a transaction with the hash: {}",
                                                val.hash.clone()
                                            );

                                            val
                                        } else {
                                            return;
                                        };
                                    let hash = tx.hash;

                                    // Try to get a lock on the runtime so we can put the tx in the database
                                    if let Ok(mut rt) = self.runtime.write() {
                                        // If we haven't a single node in the graph, we'll just treat this node as the root
                                        if rt.ledger.nodes.is_empty() {
                                            // Just push the root node onto the graph
                                            rt.ledger.push(tx, None);
                                        } else {
                                            // Make a proposal for the transaction, so we can execute it more effectively
                                            let proposal = Proposal::new(
                                                "sync_child".to_owned(),
                                                ProposalData::new(
                                                    "ledger::transactions".to_owned(),
                                                    Operation::Append {
                                                        value_to_append: record.value,
                                                    },
                                                ),
                                            );

                                            // The ID of the proposal. We need to copy this, since we'll move it into the system through registration
                                            let id = proposal.proposal_id;

                                            // Put the proposal in the system, so we can execute it
                                            rt.push_proposal(proposal);

                                            // Execute the proposal so it gets added to the dag
                                            match rt.execute_proposal(id) {
                                                Ok(_) => {
                                                    info!("Successfully executed transaction {}", id)
                                                }
                                                Err(e) => warn!(
                                                    "Failed to execute transaction {}: {}",
                                                    hash, e
                                                ),
                                            }
                                        }
                                    }

                                    // Get a quorum to poll at least 50% of the network
                                    let q: Quorum = self.active_subset_quorum();

                                    info!("Fetching the next transaction in the DAG...");

                                    // Get the next hash in the dag
                                    self.kad_dht
                                        .get_record(&Key::new(&sync::next_transaction_key(hash)), q);
                                } else if String::from_utf8_lossy(record.key.as_ref())
                                    .contains("ledger::transactions::next")
                                {
                                    // Try to convert the raw bytes into an actual hash
                                    let hash: Hash = Hash::new(record.value);

                                    info!("Determined the next hash in the remote DAG: {}", hash);

                                    // Get a quorum to poll at least 50% of the network
                                    let q: Quorum = self.active_subset_quorum();

                                    // Get the actual transaction corresponding to what we now know is the hash of such a transaction
                                    self.kad_dht.get_record(
                                        &Key::new(&sync::transaction_with_hash_key(hash)),
                                        q,
                                    );
                                }
                            }
                        }
                    }
                }

                // An error occurred while fetching the record; print it
                QueryResult::GetRecord(Err(e)) => {
                    debug!("Failed to load record: {:?}", e);
                }

                // The record was successfully set; print out the record name
                QueryResult::PutRecord(Ok(result)) => {
                    // Print out the successful set operation
                    debug!(
                        "Set key successfully: {}",
                        String::from_utf8_lossy(result.key.as_ref())
                    );
                }

                // An error occurred while fetching the record; print it
                QueryResult::PutRecord(Err(e)) => {
                    debug!("Failed to set key: {:?}", e);
                    self.should_broadcast_dag = true;
                }

                _ => {}
            }
            _ => {}
        }
    }
}
