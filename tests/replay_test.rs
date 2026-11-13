//! Replay prevention tests.

use blvm_mesh::payment_proof::PaymentProof;
use blvm_mesh::replay::ReplayPrevention;

fn peer(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn ln_proof(expires_at: u64) -> PaymentProof {
    PaymentProof::Lightning {
        invoice: "lnbc1pstub".to_string(),
        preimage: [1u8; 32],
        amount_msats: 1000,
        timestamp: 1,
        expires_at,
    }
}

#[test]
fn payment_proof_replay_rejected_on_second_use() {
    let replay = blvm_mesh::replay::ReplayPrevention::new(3600);
    let proof_a = ln_proof(u64::MAX);
    let proof_b = PaymentProof::Lightning {
        invoice: "lnbc1pstub2".to_string(),
        preimage: [2u8; 32],
        amount_msats: 1000,
        timestamp: 1,
        expires_at: u64::MAX,
    };
    let p = peer(2);

    replay.check_replay(&proof_a, &p, 5).unwrap();
    let err = replay.check_replay(&proof_a, &p, 6).unwrap_err();
    assert!(err.contains("replay"));
    replay.check_replay(&proof_b, &p, 7).unwrap();
}

#[test]
fn a_new_timestamp_does_not_make_a_new_on_chain_payment() {
    let replay = ReplayPrevention::new(3600);
    let p = peer(3);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let first = PaymentProof::OnChainSettlement {
        payment_request_id: "req".to_string(),
        tx_hash: [9u8; 32],
        amount_sats: 100,
        timestamp: now,
    };
    let restamped = PaymentProof::OnChainSettlement {
        payment_request_id: "other".to_string(),
        tx_hash: [9u8; 32],
        amount_sats: 1_000_000,
        timestamp: now.saturating_sub(10),
    };
    replay.check_replay(&first, &p, 1).unwrap();
    let err = replay.check_replay(&restamped, &p, 2).unwrap_err();
    assert!(err.contains("replay"));
}

#[test]
fn replay_rejects_out_of_order_sequence() {
    let replay = ReplayPrevention::new(3600);
    let proof_a = ln_proof(u64::MAX);
    let proof_b = PaymentProof::Lightning {
        invoice: "lnbc1pstub2".to_string(),
        preimage: [2u8; 32],
        amount_msats: 1000,
        timestamp: 1,
        expires_at: u64::MAX,
    };
    let p = peer(2);

    replay.check_replay(&proof_a, &p, 5).unwrap();
    let err = replay.check_replay(&proof_b, &p, 5).unwrap_err();
    assert!(err.contains("Sequence"));
}
