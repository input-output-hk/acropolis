Validate transactions phase 1
=============================

Haskell sources. Shelley epoch UTxO rule
----------------------------------------

1. Transaction validation takes place in ledger, in file
`shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs`

Validation is performed in rule "UTXO" ("PPUP"), in function
`utxoInductive`

This is the context of validation rules:
```
  TRC (UtxoEnv slot pp certState, utxos, tx) <- judgmentContext
  let utxo = utxos ^. utxoL
      UTxOState _ _ _ ppup _ _ = utxos
      txBody = tx ^. bodyTxL
      outputs = txBody ^. outputsTxBodyL
      genDelegs = dsGenDelegs (certState ^. certDStateL)
  netId <- liftSTS $ asks networkId
  -- process Protocol Parameter Update Proposals
  ppup' <-
    trans @(EraRule "PPUP" era) $ TRC (PPUPEnv slot pp genDelegs, ppup, txBody ^. updateTxBodyL)
```

* Values `utxo`, `ppup`, `genDelegs` require knowledge of `judgementContext`,
that is previous state of Ledger.
* `pp` is parameters state, already sent.

The following sub-functions are called there:
```
  {- txttl txb ≥ slot -}
  runTest $ validateTimeToLive txBody slot

  {- txins txb ≠ ∅ -}
  runTest $ validateInputSetEmptyUTxO txBody

  {- minfee pp tx ≤ txfee txb -}
  runTest $ validateFeeTooSmallUTxO pp tx utxo

  {- txins txb ⊆ dom utxo -}
  runTest $ validateBadInputsUTxO utxo $ txBody ^. inputsTxBodyL

  netId <- liftSTS $ asks networkId

  {- ∀(_ → (a, _)) ∈ txouts txb, netId a = NetworkId -}
  runTest $ validateWrongNetwork netId outputs

  {- ∀(a → ) ∈ txwdrls txb, netId a = NetworkId -}
  runTest $ validateWrongNetworkWithdrawal netId txBody

  {- consumed pp utxo txb = produced pp poolParams txb -}
  runTest $ validateValueNotConservedUTxO pp utxo certState txBody

  -- process Protocol Parameter Update Proposals
  ppup' <-
    trans @(EraRule "PPUP" era) $ TRC (PPUPEnv slot pp genDelegs, ppup, txBody ^. updateTxBodyL)

  {- ∀(_ → (_, c)) ∈ txouts txb, c ≥ (minUTxOValue pp) -}
  runTest $ validateOutputTooSmallUTxO pp outputs

  {- ∀ ( _ ↦ (a,_)) ∈ txoutstxb,  a ∈ Addrbootstrap → bootstrapAttrsSize a ≤ 64 -}
  runTest $ validateOutputBootAddrAttrsTooBig outputs

  {- txsize tx ≤ maxTxSize pp -}
  runTest $ validateMaxTxSizeUTxO pp tx
```

2. The following checks require knowledge of ledger:


```
  {- txins txb ⊆ dom utxo -}
  runTest $ validateBadInputsUTxO utxo $ txBody ^. inputsTxBodyL
```

where in `cardano-ledger-core/src/Cardano/Ledger/TxIn.hs` and
`cardano-ledger-core/src/Cardano/Ledger/State/UTxO.hs`:

```
-- | A unique ID of a transaction, which is computable from the transaction.
newtype TxId = TxId {unTxId :: SafeHash EraIndependentTxBody}
  deriving (Show, Eq, Ord, Generic)
  deriving newtype (NoThunks, ToJSON, FromJSON, HeapWords, EncCBOR, DecCBOR, NFData, MemPack)

-- | The input of a UTxO.
data TxIn = TxIn !TxId {-# UNPACK #-} !TxIx
  deriving (Generic, Eq, Ord, Show)

-- | The unspent transaction outputs.
newtype UTxO era = UTxO {unUTxO :: Map.Map TxIn (TxOut era)}
  deriving (Default, Generic, Semigroup)
```

Shelley checks for UTXOW, rule "UTXOW"
--------------------------------------

```
  -- * Individual validation steps
  validateFailedNativeScripts,
  validateMissingScripts,
  validateVerifiedWits,
  validateMetadata,
  validateMIRInsufficientGenesisSigs,
  validateNeededWitnesses,
```

```
transitionRulesUTXOW = do
  (TRC (utxoEnv@(UtxoEnv _ pp certState), u, tx)) <- judgmentContext

  {-  (utxo,_,_,_ ) := utxoSt  -}
  {-  witsKeyHashes := { hashKey vk | vk ∈ dom(txwitsVKey txw) }  -}
  let utxo = utxosUtxo u
      witsKeyHashes = witsFromTxWitnesses tx
      scriptsProvided = getScriptsProvided utxo tx

  -- check scripts
  {-  ∀ s ∈ range(txscripts txw) ∩ Scriptnative), runNativeScript s tx   -}

  runTestOnSignal $ validateFailedNativeScripts scriptsProvided tx

  {-  { s | (_,s) ∈ scriptsNeeded utxo tx} = dom(txscripts txw)          -}
  let scriptsNeeded = getScriptsNeeded utxo (tx ^. bodyTxL)
  runTest $ validateMissingScripts scriptsNeeded scriptsProvided

  -- check VKey witnesses
  {-  ∀ (vk ↦ σ) ∈ (txwitsVKey txw), V_vk⟦ txbodyHash ⟧_σ                -}
  runTestOnSignal $ validateVerifiedWits tx

  {-  witsVKeyNeeded utxo tx genDelegs ⊆ witsKeyHashes                   -}
  runTest $ validateNeededWitnesses witsKeyHashes certState utxo (tx ^. bodyTxL)

  -- check metadata hash
  {-  ((adh = ◇) ∧ (ad= ◇)) ∨ (adh = hashAD ad)                          -}
  runTestOnSignal $ validateMetadata pp tx

  -- check genesis keys signatures for instantaneous rewards certificates
  {-  genSig := { hashKey gkey | gkey ∈ dom(genDelegs)} ∩ witsKeyHashes  -}
  {-  { c ∈ txcerts txb ∩ TxCert_mir} ≠ ∅  ⇒ (|genSig| ≥ Quorum) ∧ (d pp > 0)  -}
  let genDelegs = dsGenDelegs (certState ^. certDStateL)
  coreNodeQuorum <- liftSTS $ asks quorum
  runTest $
    validateMIRInsufficientGenesisSigs genDelegs coreNodeQuorum witsKeyHashes tx

  trans @(EraRule "UTXO" era) $ TRC (utxoEnv, u, tx)
```


Some notes from consensus meeting
---------------------------------

applyTx     -- 1st phase                --- applyShelleyTx
                                            dig into applyShelleyBaseTx
                                                     applyAlonzoBasedTx
            -- 2nd phase

           Core.Tx ==> has 'isValid' code for transaction
    defaultApplyShelleyBasedTx (basic function from Shelley?)

reapplyShelleyTx -- skips some checks (e.g. signatures, 
if user keys didn't change; does not scripts again); we see
validity interval
    * Applied same transactions to different Ledger state
    * If my selection changes, I need to revalidate all
    transactions.

everything applies to Shelley. 
Either Byron, or Shelley.

Block diagram of a full block (Ouroboros consensus)
... intersect-mbo.org ...
