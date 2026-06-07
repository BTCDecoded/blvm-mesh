//! CTV / InstantSettlement verifier tests.

#![cfg(feature = "ctv")]

use blvm_mesh::payment_proof::PaymentProof;
use blvm_mesh::test_support::TestNodeAPI;
use blvm_mesh::verifier::PaymentVerifier;
use blvm_node::payment::covenant::CovenantEngine;
use blvm_protocol::payment::PaymentOutput;
use std::sync::Arc;

#[tokio::test]
async fn instant_settlement_covenant_verifies() {
    let engine = CovenantEngine::new();
    let outputs = vec![PaymentOutput {
        script: vec![0x51, 0x87],
        amount: Some(50_000),
    }];
    let proof = engine
        .create_payment_covenant("pay-ctv-1", &outputs, None)
        .expect("covenant");
    let bytes = bincode::serialize(&proof).unwrap();

    let verifier = PaymentVerifier::new(Arc::new(TestNodeAPI::default()));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let payment = PaymentProof::InstantSettlement {
        covenant_proof: bytes,
        output_index: 0,
        merkle_proof: vec![],
        amount_sats: 50_000,
        timestamp: now,
    };

    assert!(verifier.verify(&payment).await.unwrap().verified);
}

#[tokio::test]
async fn instant_settlement_wrong_amount_rejected() {
    let engine = CovenantEngine::new();
    let outputs = vec![PaymentOutput {
        script: vec![0x51, 0x87],
        amount: Some(50_000),
    }];
    let proof = engine
        .create_payment_covenant("pay-ctv-2", &outputs, None)
        .expect("covenant");
    let bytes = bincode::serialize(&proof).unwrap();

    let verifier = PaymentVerifier::new(Arc::new(TestNodeAPI::default()));
    let payment = PaymentProof::InstantSettlement {
        covenant_proof: bytes,
        output_index: 0,
        merkle_proof: vec![],
        amount_sats: 1,
        timestamp: 1,
    };

    assert!(!verifier.verify(&payment).await.unwrap().verified);
}
