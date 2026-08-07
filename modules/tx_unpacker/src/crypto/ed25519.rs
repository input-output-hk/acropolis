use acropolis_common::{crypto::verify_ed25519_signature_strict, VKeyWitness};

pub fn verify_ed25519_signature(witness: &VKeyWitness, data_to_verify: &[u8]) -> bool {
    verify_ed25519_signature_strict(
        witness.vkey.as_inner(),
        witness.signature.as_inner(),
        data_to_verify,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{Signature, VKey};
    use std::str::FromStr;

    #[test]
    fn verify_signature_correctly() {
        let vkey =
            VKey::from_str("fbc53e7aa4e5497d8662e8f0d5337441f629d1f237217bc24ac41bb6de89f841")
                .unwrap();
        let signature = Signature::from_str("3ae0dfde0fdb6e15373b274e847390ebb26a777dcaefa06f7f0938cd20268cacb9fa6080be35507361c830b44cae481191d635d2917828f303b62b487a8e0d0c").unwrap();
        let witness = VKeyWitness::new(vkey, signature);
        let message =
            hex::decode("b558c32b54cf4a59afbace53aeaed2b0578b1052e3bb58b5c12ae6eab1c5302f")
                .unwrap();
        assert!(verify_ed25519_signature(&witness, &message));
    }

    /// The 12 test vectors from "Taming the Many EdDSAs" (Chalkias, Garillot,
    /// Nikolaenko; SSR'20, <https://eprint.iacr.org/2020/1244>), verbatim from
    /// <https://github.com/novifinancial/ed25519-speccheck> cases.json.
    const SPECCHECK_VECTORS: &[(&str, &str, &str, bool)] = &[
        // 0: small-order A and R, S = 0
        (
            "8c93255d71dcab10e8f379c26200f3c7bd5f09d9bc3068d3ef4edeb4853022b6",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a0000000000000000000000000000000000000000000000000000000000000000",
            false,
        ),
        // 1: small-order a only
        (
            "9bd9f44f4dcc75bd531b56b2cd280b0bb38fc1cd6d1230e14861d861de092e79",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa",
            "f7badec5b8abeaf699583992219b7b223f1df3fbbea919844e3f7c554a43dd43a5bb704786be79fc476f91d3f3f89b03984d8068dcf1bb7dfc6637b45450ac04",
            false,
        ),
        // 2: small-order r only
        (
            "aebf3f2601a0c8c5d39cc7d8911642f740b78168218da8471772b35f9d35b9ab",
            "f7badec5b8abeaf699583992219b7b223f1df3fbbea919844e3f7c554a43dd43",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa8c4bd45aecaca5b24fb97bc10ac27ac8751a7dfe1baff8b953ec9f5833ca260e",
            false,
        ),
        // 3: mixed-order a and r; passes both equations
        (
            "9bd9f44f4dcc75bd531b56b2cd280b0bb38fc1cd6d1230e14861d861de092e79",
            "cdb267ce40c5cd45306fa5d2f29731459387dbf9eb933b7bd5aed9a765b88d4d",
            "9046a64750444938de19f227bb80485e92b83fdb4b6506c160484c016cc1852f87909e14428a7a1d62e9f22f3d3ad7802db02eb2e688b6c52fcd6648a98bd009",
            true,
        ),
        // 4: mixed-order a and r; cofactored accepts, cofactorless rejects
        (
            "e47d62c63f830dc7a6851a0b1f33ae4bb2f507fb6cffec4011eaccd55b53f56c",
            "cdb267ce40c5cd45306fa5d2f29731459387dbf9eb933b7bd5aed9a765b88d4d",
            "160a1cb0dc9c0258cd0a7d23e94d8fa878bcb1925f2c64246b2dee1796bed5125ec6bc982a269b723e0668e540911a9a6a58921d6925e434ab10aa7940551a09",
            false,
        ),
        // 5: mixed-order a, order-l r; pre-reduced scalar
        (
            "e47d62c63f830dc7a6851a0b1f33ae4bb2f507fb6cffec4011eaccd55b53f56c",
            "cdb267ce40c5cd45306fa5d2f29731459387dbf9eb933b7bd5aed9a765b88d4d",
            "21122a84e0b5fca4052f5b1235c80a537878b38f3142356b2c2384ebad4668b7e40bc836dac0f71076f9abe3a53f9c03c1ceeeddb658d0030494ace586687405",
            false,
        ),
        // 6: s > l
        (
            "85e241a07d148b41e47d62c63f830dc7a6851a0b1f33ae4bb2f507fb6cffec40",
            "442aad9f089ad9e14647b1ef9099a1ff4798d78589e66f28eca69c11f582a623",
            "e96f66be976d82e60150baecff9906684aebb1ef181f67a7189ac78ea23b6c0e547f7690a0e2ddcd04d87dbc3490dc19b3b3052f7ff0538cb68afb369ba3a514",
            false,
        ),
        // 7: s >> l
        (
            "85e241a07d148b41e47d62c63f830dc7a6851a0b1f33ae4bb2f507fb6cffec40",
            "442aad9f089ad9e14647b1ef9099a1ff4798d78589e66f28eca69c11f582a623",
            "8ce5b96c8f26d0ab6c47958c9e68b937104cd36e13c33566acd2fe8d38aa19427e71f98a473474f2f13f06f97c20d58cc3f54b8bd0d272f42b695dd7e89a8c22",
            false,
        ),
        // 8: non-canonical r, reduced for hash
        (
            "9bedc267423725d473888631ebf45988bad3db83851ee85c85e241a07d148b41",
            "f7badec5b8abeaf699583992219b7b223f1df3fbbea919844e3f7c554a43dd43",
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff03be9678ac102edcd92b0210bb34d7428d12ffc5df5f37e359941266a4e35f0f",
            false,
        ),
        // 9: non-canonical r, not reduced for hash
        (
            "9bedc267423725d473888631ebf45988bad3db83851ee85c85e241a07d148b41",
            "f7badec5b8abeaf699583992219b7b223f1df3fbbea919844e3f7c554a43dd43",
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffca8c5b64cd208982aa38d4936621a4775aa233aa0505711d8fdcfdaa943d4908",
            false,
        ),
        // 10: non-canonical a, reduced for hash
        (
            "e96b7021eb39c1a163b6da4e3093dcd3f21387da4cc4572be588fafae23c155b",
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "a9d55260f765261eb9b84e106f665e00b867287a761990d7135963ee0a7d59dca5bb704786be79fc476f91d3f3f89b03984d8068dcf1bb7dfc6637b45450ac04",
            false,
        ),
        // 11: non-canonical a, not reduced for hash
        (
            "39a591f5321bbe07fd5a23dc2f39d025d74526615746727ceefd6e82ae65c06f",
            "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "a9d55260f765261eb9b84e106f665e00b867287a761990d7135963ee0a7d59dca5bb704786be79fc476f91d3f3f89b03984d8068dcf1bb7dfc6637b45450ac04",
            false,
        ),
    ];

    #[test]
    fn witness_verification_matches_cardano_node() {
        let mut diverged = Vec::new();
        for (i, (msg, pk, sig, cardano_accepts)) in SPECCHECK_VECTORS.iter().enumerate() {
            let msg = hex::decode(msg).unwrap();
            let witness = VKeyWitness::new(
                VKey::from_str(pk).unwrap(),
                Signature::from_str(sig).unwrap(),
            );
            let ours = verify_ed25519_signature(&witness, &msg);
            if ours != *cardano_accepts {
                diverged.push((i, *cardano_accepts, ours));
            }
        }
        assert!(
            diverged.is_empty(),
            "diverged from cardano-node on {} vector(s): {:?} (idx, cardano-node, ours)",
            diverged.len(),
            diverged
        );
    }

    #[test]
    fn cryptoxide_diverges_from_cardano_node_as_documented() {
        let mut diverged = Vec::new();
        for (i, (msg, pk, sig, cardano_accepts)) in SPECCHECK_VECTORS.iter().enumerate() {
            let msg = hex::decode(msg).unwrap();
            let pk: [u8; 32] = hex::decode(pk).unwrap().try_into().unwrap();
            let sig: [u8; 64] = hex::decode(sig).unwrap().try_into().unwrap();
            if cryptoxide::ed25519::verify(&msg, &pk, &sig) != *cardano_accepts {
                diverged.push(i);
            }
        }
        assert_eq!(
            diverged,
            vec![0, 1, 2, 11],
            "cryptoxide's divergence from cardano-node changed; expected vectors \
             [0, 1, 2, 11] (small-order public key or R component)"
        );
    }

    /// A = R = identity point with S = 0 satisfies the cofactorless equation for
    /// EVERY message, so a permissive verifier accepts this one fixed pair
    /// against any transaction body hash -- no private key, no search. Because
    /// Cardano verifies every witness in the set rather than only the required
    /// ones, an attacker could append such a witness to an ordinary transaction.
    /// cardano-node rejects it: the vkey is in libsodium's small-order blacklist.
    #[test]
    fn rejects_universal_small_order_forgery() {
        let witness = VKeyWitness::new(
            VKey::from_str("0100000000000000000000000000000000000000000000000000000000000000")
                .unwrap(),
            Signature::from_str(&format!("01{}", "00".repeat(63))).unwrap(),
        );
        for i in 0u32..1000 {
            let tx_body_hash = acropolis_common::crypto::keyhash_256(&i.to_le_bytes());
            assert!(
                !verify_ed25519_signature(&witness, tx_body_hash.as_ref()),
                "accepted a small-order forgery at i={i}"
            );
        }

        for i in 0u32..1000 {
            let tx_body_hash = acropolis_common::crypto::keyhash_256(&i.to_le_bytes());
            assert!(
                cryptoxide::ed25519::verify(tx_body_hash.as_ref(), &witness.vkey, &witness.signature),
                "rejected a small-order forgery at i={i}"
            );
        }
    }
}
