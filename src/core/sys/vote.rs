use ed25519_dalek; // Import the edwards25519 digital signature library

use bincode; // Import serde bincode
use serde::{Deserialize, Serialize}; // Import serde serialization

use super::super::super::crypto::hash; // Import the hash primitive
use super::super::types::signature; // Import the signature primitive

use blake3::Hasher as BlakeHasher;
use std::{
    fmt,
    hash::{Hash, Hasher},
};

/// A binary, signed vote regarding a particular proposal.
#[derive(Serialize, Deserialize, Clone)]
pub struct Vote {
    /// The hash of the target proposal
    pub target_proposal: hash::Hash,
    /// Whether the voter is in favor of the particular proposal or not
    pub in_favor: bool,
    /// The signature of the voter
    pub signature: Option<signature::Signature>,
}

/// Implement a set of voting helper methods.
impl Vote {
    /// Initialize and sign a new vote instance.
    pub fn new(
        proposal_id: hash::Hash,
        in_favor: bool,
        signature_keypair: ed25519_dalek::Keypair,
    ) -> Vote {
        let mut vote: Vote = Vote {
            target_proposal: proposal_id, // Set proposal ID
            in_favor,                     // Set in favor of proposal
            signature: None,              // No signature yet
        }; // Initialize vote

        if let Ok(serialized_vote) = bincode::serialize(&vote) {
            // Serialize vote
            vote.signature = Some(signature::Signature {
                public_key_bytes: bincode::serialize(&signature_keypair.public).unwrap_or_default(),
                signature_bytes: bincode::serialize(
                    &signature_keypair.sign(vote.hash(&mut BlakeHasher::new())),
                )
                .unwrap_or_default(),
            }); // Set signature
        }

        vote // Return initialized vote
    }

    /// Ensures that the signature associated with the vote is authentic.
    pub fn valid(&self) -> bool {
        // Ensure that the vote has a signature attached to it
        let sig = if let Some(signature) = self.signature {
            signature
        } else {
            // The vote must be invalid, since it doesn't even have a signature
            return false;
        };

        // Ensure that the signature is valid, considering the vote's hash
        sig.verify(self.hash())
    }
}

impl fmt::Display for Vote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // If the vote is in favor, express such an agreement as a string by saying "in favor of
        // prop"
        write!(
            f,
            "{}",
            if self.in_favor {
                format!("in favor of proposal {}", self.target_proposal)
            } else {
                format!("in opposition to proposal {}", self.target_proposal)
            }
        )
    }
}

impl Hash for Vote {
    /// Hashes the vote using the stdlib hasher.
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Copy and remove the signature from the vote; this will preserve its original contents
        let mut to_be_hashed = self.clone();
        to_be_hashed.signature = None;

        // Hash the contents of the vote
        self.target_proposal.hash(state);
        self.in_favor.hash(state);
    }
}
