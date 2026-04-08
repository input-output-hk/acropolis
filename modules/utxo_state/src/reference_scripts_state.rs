use acropolis_common::{ReferenceScript, ScriptHash};
use imbl::HashMap as ImblHashMap;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ReferenceScriptsState {
    /// <script hash, (ref script struct, and its occurrence count)>
    reference_scripts: ImblHashMap<ScriptHash, (ReferenceScript, u64)>,
}

impl ReferenceScriptsState {
    pub fn apply_reference_scripts(
        &mut self,
        spent_reference_scripts: &[ScriptHash],
        created_reference_scripts: &[(ScriptHash, ReferenceScript)],
    ) {
        for script_hash in spent_reference_scripts {
            if let Some((_, count)) = self.reference_scripts.get_mut(script_hash) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.reference_scripts.remove(script_hash);
                }
            }
        }

        for (script_hash, reference_script) in created_reference_scripts {
            self.reference_scripts
                .entry(*script_hash)
                .or_insert((reference_script.clone(), 0))
                .1 += 1;
        }
    }

    pub fn lookup_reference_script(&self, script_hash: &ScriptHash) -> Option<ReferenceScript> {
        self.reference_scripts.get(script_hash).map(|(script, _)| script.clone())
    }
}
