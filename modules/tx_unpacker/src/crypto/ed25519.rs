use acropolis_common::VKeyWitness;
use cryptoxide::ed25519;

pub fn verify_ed25519_signature(witness: &VKeyWitness, data_to_verify: &[u8]) -> bool {
    ed25519::verify(
        data_to_verify,
        witness.vkey.as_inner(),
        witness.signature.as_inner(),
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
}
