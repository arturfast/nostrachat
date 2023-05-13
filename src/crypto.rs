use std::sync::{ Arc, Mutex };
use hkdf::Hkdf;
//use rand::rngs::SmallRng;
use sha2::Sha256;

//use secp256k1::{ KeyPair, ecdh::SharedSecret, Secp256k1, rand::rngs::OsRng, PublicKey };
use nostr::prelude::secp256k1::SecretKey;
use nostr::prelude::secp256k1::ecdh::SharedSecret;
use nostr::prelude::secp256k1::PublicKey;
use nostr::key::Keys;
use nostr::prelude::XOnlyPublicKey;
use nostr::prelude::Parity;

use hex::encode;

#[derive(Clone)]
pub struct RatchetProfile {
    chain_key: [u8; 32],
    pub ephemeral_keys: Arc::<Mutex::<EphemeralKeyPair>>
}

impl RatchetProfile {

    pub fn new(secret_key: SecretKey, recipient_public_key: PublicKey) -> Self {

        let shared_secret = SharedSecret::new(&recipient_public_key, &secret_key);
        let (chain_key, _) = Hkdf::<Sha256>::extract(None, &shared_secret.secret_bytes());
        RatchetProfile {
            chain_key: chain_key.into(),
            ephemeral_keys: Arc::new(Mutex::new(EphemeralKeyPair { secret_key: secret_key, recipient_public_key: recipient_public_key})),
        }
    }

    pub fn rotate(&mut self) -> [u8; 256] {
        let (chain_key, ratchet) = Hkdf::<Sha256>::extract(None, &self.chain_key);
        self.chain_key = chain_key.into();
        let mut okm = [0u8; 256];
        let recipient_public_key = self.ephemeral_keys.lock().unwrap().recipient_public_key;
        let secret_key = self.ephemeral_keys.lock().unwrap().secret_key;
        let shared_secret = SharedSecret::new(&recipient_public_key, &secret_key);
        // Debugging
/*        println!("RECP PUBKEY (ROTATE): {:?}", recipient_public_key.serialize_uncompressed());
        println!("SEC KEY (ROTATE): {:?}", secret_key.secret_bytes());
        println!("PUBKEY OUT OF SEC_KEY EVEN (ROTATE): {:?}", Keys::new(secret_key).public_key().public_key(Parity::Even).serialize_uncompressed());
        println!("PUBKEY OUT OF SEC_KEY ODD (ROTATE): {:?}", Keys::new(secret_key).public_key().public_key(Parity::Odd).serialize_uncompressed());
        println!("PUBKEY OUT OF SEC_KEY NORMALIZED (ROTATE): {:?}", Keys::new(secret_key).normalized_public_key().unwrap().serialize_uncompressed()); 
        println!("SHARED SECRET (ROTATE): {:?}", shared_secret.secret_bytes());
        println!("SHARED SECRET DISPLAY SECRET (ROTATE): {:?}", shared_secret.display_secret());
*/

        ratchet.expand(&shared_secret.secret_bytes(), &mut okm);
        okm
    }

    pub fn encrypt_message(&mut self, input: String) -> String {
        let message_key = self.rotate();
        hex::encode(message_key)
    }

    pub fn decrypt_message(&mut self, input: String) -> String {
        let message_key = self.rotate();
        input + &hex::encode(message_key)
    }
}

pub struct EphemeralKeyPair {
    pub recipient_public_key: PublicKey,
    pub secret_key: SecretKey,
}
