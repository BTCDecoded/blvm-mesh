//! Helpers for building valid BOLT11 invoices in tests.

use bitcoin::hashes::{sha256, Hash, HashEngine};
use bitcoin::secp256k1::{self, ecdsa::RecoverableSignature};
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use std::time::Duration;

/// Build a signed test invoice + matching preimage for mesh verifier tests.
pub fn test_lightning_invoice(amount_msats: u64, preimage: [u8; 32]) -> String {
    let mut engine = sha256::Hash::engine();
    engine.input(&preimage);
    let payment_hash = sha256::Hash::from_engine(engine);

    InvoiceBuilder::new(Currency::Bitcoin)
        .description("blvm-mesh test".to_owned())
        .amount_milli_satoshis(amount_msats)
        .payment_hash(payment_hash)
        .payment_secret(PaymentSecret([1u8; 32]))
        .duration_since_epoch(Duration::from_secs(1_700_000_000))
        .min_final_cltv_expiry_delta(144)
        .expiry_time(Duration::from_secs(3600))
        .build_signed(|_| {
            RecoverableSignature::from_compact(
                &[
                    0x38, 0xec, 0x68, 0x91, 0x34, 0x5e, 0x20, 0x41, 0x45, 0xbe, 0x8a, 0x3a, 0x99,
                    0xde, 0x38, 0xe9, 0x8a, 0x39, 0xd6, 0xa5, 0x69, 0x43, 0x4e, 0x18, 0x45, 0xc8,
                    0xaf, 0x72, 0x05, 0xaf, 0xcf, 0xcc, 0x7f, 0x42, 0x5f, 0xcd, 0x14, 0x63, 0xe9,
                    0x3c, 0x32, 0x88, 0x1e, 0xad, 0x0d, 0x6e, 0x35, 0x6d, 0x46, 0x7e, 0xc8, 0xc0,
                    0x25, 0x53, 0xf9, 0xaa, 0xb1, 0x5e, 0x57, 0x38, 0xb1, 0x1f, 0x12, 0x7f,
                ],
                secp256k1::ecdsa::RecoveryId::from_i32(0).expect("recovery id"),
            )
            .expect("signature")
        })
        .expect("invoice build")
        .to_string()
}
