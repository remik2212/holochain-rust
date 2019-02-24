#![allow(warnings)]
use holochain_sodium::{kx, secbuf::SecBuf, sign};
use holochain_sodium::*;

use crate::{
    keypair::*,
    password_encryption::{self, PwHashConfig, EncryptedData},
};
use holochain_core_types::error::{HcResult, HolochainError};
use rustc_serialize::json;
use std::str;

use serde_derive::{Deserialize, Serialize};

const BLOB_FORMAT_VERSION: u8 = 2;

const BLOB_DATA_LEN_MISALIGN: usize = 1 // version byte
    + sign::PUBLICKEYBYTES
    + kx::PUBLICKEYBYTES
    + sign::SECRETKEYBYTES
    + kx::SECRETKEYBYTES;

pub const BLOB_DATA_LEN: usize = ((BLOB_DATA_LEN_MISALIGN + 8 - 1) / 8) * 8;


/// The data includes a base32 encoded string of the ReturnBlobData that was created by combining all the keys in one SecBuf
#[derive(Serialize, Deserialize)]
pub struct KeyBlob {
    pub seed_type: SeedType,
    pub hint: String,
    // encoded / serialized?
    pub data: String,
}


/// Enum holding all the types of seeds that can generate cryptographic keys
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum SeedType {
    Root,
    Revocation,
    Device,
    DevicePin,
    Application,
}

/// Struct holding all the keys generated by a seed

pub struct KeyBundle {
    pub sign_keys: SigningKeyPair,
    pub enc_keys: EncryptingKeyPair,
    pub seed_type: SeedType,
}

impl KeyBundle {
    /// Derive the keys from a 32 bytes seed buffer
    /// @param {SecBuf} seed - the seed buffer
    pub fn new_from_seed(seed: &mut SecBuf, seed_type: SeedType) -> Result<Self, HolochainError> {
        assert_eq!(seed.len(), SEED_SIZE);
        Ok(KeyBundle {
            sign_keys: SigningKeyPair::new_from_seed(seed)?,
            enc_keys: EncryptingKeyPair::new_from_seed(seed)?,
            seed_type,
        })
    }

    /// Construct the pairs from an encrypted blob
    /// @param {object} bundle - persistence info
    /// @param {SecBuf} passphrase - decryption passphrase
    /// @param {string} config - Settings for pwhash
    pub fn from_blob(
        blob: &KeyBlob,
        passphrase: &mut SecBuf,
        config: Option<PwHashConfig>,
    ) -> HcResult<KeyBundle> {
        // decoding the blob.data of type EncryptedData
        let blob_decoded = base64::decode(&blob.data)?;

        // Deserialize
        let blob_string = str::from_utf8(&blob_decoded).unwrap();
        let data: EncryptedData = json::decode(&blob_string).unwrap();
        // Decrypt
        let mut decrypted_data = SecBuf::with_secure(BLOB_DATA_LEN);
        password_encryption::pw_dec(&data, passphrase, &mut decrypted_data, config)?;

        let mut priv_sign = SecBuf::with_secure(SIGNATURE_SIZE);
        let mut priv_enc = SecBuf::with_secure(32);
        let mut pub_sign = SecBuf::with_secure(SIGNATURE_SIZE);
        let mut pub_enc = SecBuf::with_secure(32);

        // FIXME
//        let pub_keys = {
//            let decrypted_data = decrypted_data.read_lock();
//            if decrypted_data[0] != BLOB_FORMAT_VERSION {
//                return Err(HolochainError::ErrorGeneric(format!(
//                    "Invalid Blob Format: v{:?} != v{:?}",
//                    decrypted_data[0], BLOB_FORMAT_VERSION
//                )));
//            }
//            priv_sign.write(0, &decrypted_data[65..129])?;
//            priv_enc.write(0, &decrypted_data[129..161])?;
//
//            KeyBuffer::with_raw_parts(
//                array_ref![&decrypted_data, 1, 32],
//                array_ref![&decrypted_data, 33, 32],
//            )
//                .render()
//        };


        Ok(KeyBundle {
            sign_keys: SigningKeyPair::new(SigningKeyPair::encode_pub_key(&mut pub_sign), priv_sign),
            enc_keys: EncryptingKeyPair::new(EncryptingKeyPair::encode_pub_key(&mut pub_enc), priv_enc),
            seed_type: blob.seed_type.clone(),
        })
    }

    /// Generate an encrypted blob for persistence
    /// @param {SecBuf} passphrase - the encryption passphrase
    /// @param {string} hint - additional info / description for the bundle
    /// @param {string} config - Settings for pwhash
    pub fn as_blob(
        &mut self,
        passphrase: &mut SecBuf,
        hint: String,
        config: Option<PwHashConfig>,
    ) -> HcResult<KeyBlob> {
        // let corrected_pub_keys = KeyBuffer::with_corrected(&self.pub_keys)?;
        // Initialize buffer
        let mut data_buf = SecBuf::with_secure(BLOB_DATA_LEN);
        let mut offset: usize = 0;
        // Write version
        data_buf.write(offset, &[BLOB_FORMAT_VERSION])?;
        offset += 1;
        // Write public signing key
        data_buf.write(offset, &self.sign_keys.decode_pub_key())?;
        offset += sign::PUBLICKEYBYTES;
        // Write public signing key
        data_buf.write(offset, &self.enc_keys.decode_pub_key())?;
        offset += kx::PUBLICKEYBYTES;
        // Write public signing key
        data_buf.write(offset, &**self.sign_keys.keypair.private.read_lock())?;
        offset += sign::SECRETKEYBYTES;
        // Write public signing key
        data_buf.write(offset, &**self.sign_keys.keypair.private.read_lock())?;
        offset += kx::SECRETKEYBYTES;
        assert_eq!(offset, BLOB_DATA_LEN_MISALIGN);

        // encrypt buffer
        let encrypted_blob = password_encryption::pw_enc(&mut data_buf, passphrase, config)?;
        let serialized_blob = json::encode(&encrypted_blob).expect("");
        // conver to base64
        let encoded_blob = base64::encode(&serialized_blob);
        // Done
        Ok(KeyBlob {
            seed_type: self.seed_type.clone(),
            hint,
            data: encoded_blob,
        })
    }

    /// get the identifier key
    pub fn get_id(&self) -> Base32 {
        self.sign_keys.keypair.public.clone()
    }

    /// sign some arbitrary data with the signing private key
    /// @param {SecBuf} data - the data to sign
    /// @param {SecBuf} signature - Empty Buf to be filled with the signature
    pub fn sign(&mut self, data: &mut SecBuf, signature: &mut SecBuf) -> HcResult<()> {
        self.sign_keys.sign(data, signature)
    }

    /// verify data that was signed with our private signing key
    /// @param {SecBuf} data buffer to verify
    /// @param {SecBuf} signature candidate for that data buffer
    /// @return true if verification succeeded
    pub fn verify(&mut self, data: &mut SecBuf, signature: &mut SecBuf) -> bool {
        self.sign_keys.verify(data, signature)
    }

    ///
    pub fn is_same(&mut self, other: &mut KeyBundle) -> bool {
        self.sign_keys.keypair.is_same(&mut other.sign_keys.keypair) &&
            self.enc_keys.keypair.is_same(&mut other.enc_keys.keypair) &&
            self.seed_type == other.seed_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use holochain_sodium::pwhash;

    const TEST_CONFIG: Option<PwHashConfig> = Some(PwHashConfig(
        pwhash::OPSLIMIT_INTERACTIVE,
        pwhash::MEMLIMIT_INTERACTIVE,
        pwhash::ALG_ARGON2ID13,
    ));


    fn test_generate_random_seed() -> SecBuf {
        let mut seed = SecBuf::with_insecure(SEED_SIZE);
        seed.randomize();
        seed
    }

    fn test_generate_random_bundle() -> KeyBundle {
        let mut seed = test_generate_random_seed();
        KeyBundle::new_from_seed(&mut seed, SeedType::Root).unwrap()
    }

    #[test]
    fn it_should_create_keybundle_from_seed() {
        let bundle = test_generate_random_bundle();
        assert_eq!(SeedType::Root, bundle.seed_type);
        assert_eq!(64, bundle.sign_keys.keypair.private.len());
        assert_eq!(32, bundle.enc_keys.keypair.private.len());

        let id = bundle.get_id();
        println!("id: {:?}", id);
        assert_ne!(0, id.len());
    }

    #[test]
    fn it_should_sign_message_and_verify() {
        let mut bundle = test_generate_random_bundle();

        // Create random data
        let mut message = SecBuf::with_insecure(16);
        message.randomize();

        // sign it
        let mut message_signed = SecBuf::with_insecure(SIGNATURE_SIZE);
        bundle.sign(&mut message, &mut message_signed).unwrap();
        // authentify signature
        let succeeded = bundle.verify(&mut message_signed, &mut message);
        assert!(succeeded);

        // Create random data
        let mut random_signed = SecBuf::with_insecure(SIGNATURE_SIZE);
        random_signed.randomize();
        // authentify random signature
        let succeeded = bundle.verify(&mut random_signed, &mut message);
        assert!(!succeeded);

        // Randomize data again
        message.randomize();
        let succeeded = bundle.verify(&mut message_signed, &mut message);
        assert!(!succeeded);
    }


    #[test]
    fn it_should_blob_keybundle() {
        let mut seed = test_generate_random_seed();
        let mut passphrase = test_generate_random_seed();

        let mut bundle = KeyBundle::new_from_seed(&mut seed, SeedType::Root).unwrap();

        let blob = bundle
            .as_blob(&mut passphrase, "hint".to_string(), TEST_CONFIG)
            .unwrap();

        println!("blob.data: {}", blob.data);

        assert_eq!(SeedType::Root, blob.seed_type);
        assert_eq!("hint", blob.hint);

        let mut unblob = KeyBundle::from_blob(&blob, &mut passphrase, TEST_CONFIG).unwrap();

        assert!(bundle.is_same(&mut unblob));

//        assert_eq!(64, unblob.sign_priv.len());
//        assert_eq!(32, unblob.enc_priv.len());
//        assert_eq!(92, unblob.pub_keys.len());

        // Test with wrong passphrase
        passphrase.randomize();
        let unblob = KeyBundle::from_blob(&blob, &mut passphrase, TEST_CONFIG);

    }

    #[test]
    fn it_should_try_get_bundle_and_decode_it() {
        let mut seed = test_generate_random_seed();
        let mut passphrase = test_generate_random_seed();

        let mut bundle = KeyBundle::new_from_seed(&mut seed, SeedType::Root).unwrap();
        let mut passphrase = SecBuf::with_insecure(SEED_SIZE);

        let blob = bundle
            .as_blob(&mut passphrase, "hint".to_string(), TEST_CONFIG)
            .unwrap();


        let unblob = KeyBundle::from_blob(&blob, &mut passphrase, TEST_CONFIG).unwrap();

//        assert_eq!(64, keypair_from_bundle.sign_priv.len());
//        assert_eq!(32, keypair_from_bundle.enc_priv.len());
//        assert_eq!(92, keypair_from_bundle.pub_keys.len());
    }

    // #[test]
    // fn it_should_encode_n_decode_data() {
    //     let mut seed = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed);
    //     let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();
    //
    //     let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed_1);
    //     let mut keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();
    //
    //     let mut message = SecBuf::with_insecure(16);
    //     random_secbuf(&mut message);
    //
    //     let recipient_id = vec![&keypair_1.pub_keys];
    //
    //     let mut out = Vec::new();
    //     keypair_main
    //         .encrypt(recipient_id, &mut message, &mut out)
    //         .unwrap();
    //
    //     match keypair_1.decrypt(keypair_main.pub_keys, &mut out) {
    //         Ok(mut dm) => {
    //             let message = message.read_lock();
    //             let dm = dm.read_lock();
    //             assert_eq!(format!("{:?}", *message), format!("{:?}", *dm));
    //         }
    //         Err(_) => {
    //             assert!(false);
    //         }
    //     };
    // }
    //
    // #[test]
    // fn it_should_encode_n_decode_data_for_multiple_users2() {
    //     let mut seed = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed);
    //     let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();
    //
    //     let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed_1);
    //     let keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();
    //
    //     let mut seed_2 = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed_2);
    //     let mut keypair_2 = Keypair::new_from_seed(&mut seed_2).unwrap();
    //
    //     let mut message = SecBuf::with_insecure(16);
    //     random_secbuf(&mut message);
    //
    //     let recipient_id = vec![&keypair_1.pub_keys, &keypair_2.pub_keys];
    //
    //     let mut out = Vec::new();
    //     keypair_main
    //         .encrypt(recipient_id, &mut message, &mut out)
    //         .unwrap();
    //
    //     match keypair_2.decrypt(keypair_main.pub_keys, &mut out) {
    //         Ok(mut dm) => {
    //             let message = message.read_lock();
    //             let dm = dm.read_lock();
    //             assert_eq!(format!("{:?}", *message), format!("{:?}", *dm));
    //         }
    //         Err(_) => {
    //             assert!(false);
    //         }
    //     };
    // }
    //
    // #[test]
    // fn it_should_encode_n_decode_data_for_multiple_users1() {
    //     let mut seed = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed);
    //     let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();
    //
    //     let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed_1);
    //     let mut keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();
    //
    //     let mut seed_2 = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed_2);
    //     let keypair_2 = Keypair::new_from_seed(&mut seed_2).unwrap();
    //
    //     let mut message = SecBuf::with_insecure(16);
    //     random_secbuf(&mut message);
    //
    //     let recipient_id = vec![&keypair_1.pub_keys, &keypair_2.pub_keys];
    //
    //     let mut out = Vec::new();
    //     keypair_main
    //         .encrypt(recipient_id, &mut message, &mut out)
    //         .unwrap();
    //
    //     match keypair_1.decrypt(keypair_main.pub_keys, &mut out) {
    //         Ok(mut dm) => {
    //             println!("Decrypted Message: {:?}", dm);
    //             let message = message.read_lock();
    //             let dm = dm.read_lock();
    //             assert_eq!(format!("{:?}", *message), format!("{:?}", *dm));
    //         }
    //         Err(_) => {
    //             println!("Error");
    //             assert!(false);
    //         }
    //     };
    // }
    //
    // #[test]
    // fn it_should_with_fail_when_wrong_key_used_to_decrypt() {
    //     let mut seed = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed);
    //     let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();
    //
    //     let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed_1);
    //     let keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();
    //
    //     let mut seed_2 = SecBuf::with_insecure(SEEDSIZE);
    //     random_secbuf(&mut seed_2);
    //     let mut keypair_2 = Keypair::new_from_seed(&mut seed_2).unwrap();
    //
    //     let mut message = SecBuf::with_insecure(16);
    //     random_secbuf(&mut message);
    //
    //     let recipient_id = vec![&keypair_1.pub_keys];
    //
    //     let mut out = Vec::new();
    //     keypair_main
    //         .encrypt(recipient_id, &mut message, &mut out)
    //         .unwrap();
    //
    //     keypair_2
    //         .decrypt(keypair_main.pub_keys, &mut out)
    //         .expect_err("should have failed");
    // }
}
