-- =============================================================================
-- Cardano Rewards & SPDD Diagnostic Queries
-- =============================================================================
--
-- Purpose: Debug reward calculations and stake pool distribution discrepancies
-- Target:  cardano-db-sync database
--
-- Usage:
--   # Set the epoch you're investigating
--   export EPOCH=235
--
--   # Run all queries
--   psql "$DBSYNC_URL" -v epoch=$EPOCH -f rewards_diagnostics.sql > results.txt 2>&1
--
-- Understanding Epoch Offsets:
-- ----------------------------
-- Cardano uses a Mark/Set/Go snapshot system with specific timing:
--
--   - epoch_stake table stores snapshots 2 epochs AHEAD
--     e.g., epoch_no=237 contains the snapshot for epoch 235
--
--   - Rewards earned in epoch N are spendable in epoch N+2
--     e.g., rewards earned in epoch 233 are spendable in epoch 235
--
--   - Stability window is at 4k/5 slots = 172,800 slots into an epoch
--     Registration changes BEFORE this affect reward eligibility (pre-Babbage)
--
-- Which Epoch to Debug:
-- ---------------------
-- If your SPDD verification fails at epoch N:
--   - Query epoch_stake with epoch_no = N + 2
--   - Check rewards with earned_epoch = N - 2 (if investigating what went into SPDD)
--   - Check registration changes in epoch N (for stability window issues)
--
-- If your rewards verification fails at epoch N:
--   - Check rewards with earned_epoch = N
--   - Check epoch_stake for epoch_no = N (the "go" snapshot used for rewards)
--   - Check registration/deregistration in epochs N and N-1
--
-- Key Era Boundaries (Mainnet):
-- -----------------------------
--   - Epoch 208: Shelley start
--   - Epoch 236: Allegra start (AVVM addresses cancelled, ~299M ADA → reserves)
--   - Epoch 251: Mary start
--   - Epoch 290: Alonzo start
--   - Epoch 365: Babbage start (reward prefilter removed)
--
-- =============================================================================

-- #############################################################################
-- SECTION 1: POTS & RESERVES
-- Understanding reserves, treasury, and deposits state
-- #############################################################################

-- 1.1 Check ada_pots across epoch range (shows reserves/treasury changes)
-- Use this to identify unusual pot movements (e.g., AVVM at epoch 236)
SELECT epoch_no,
       reserves,
       treasury,
       reserves - LAG(reserves) OVER (ORDER BY epoch_no) as reserves_change,
       treasury - LAG(treasury) OVER (ORDER BY epoch_no) as treasury_change
FROM ada_pots
WHERE epoch_no BETWEEN :epoch - 2 AND :epoch + 2
ORDER BY epoch_no;

-- 1.2 Get ada_pots with deposits
SELECT epoch_no, reserves, treasury, deposits_stake
FROM ada_pots
WHERE epoch_no BETWEEN :epoch - 2 AND :epoch + 2
ORDER BY epoch_no;

-- #############################################################################
-- SECTION 2: TOTAL STAKE & EPOCH SNAPSHOTS
-- SPDD verification and stake distribution analysis
-- #############################################################################

-- 2.1 Total stake and delegator counts by epoch
-- Remember: epoch_stake stores snapshots 2 epochs ahead
SELECT epoch_no,
       epoch_no - 2            as snapshot_for_epoch,
       SUM(amount)             as total_stake,
       COUNT(*)                as delegator_count,
       COUNT(DISTINCT pool_id) as pool_count
FROM epoch_stake
WHERE epoch_no BETWEEN :epoch AND :epoch + 4
GROUP BY epoch_no
ORDER BY epoch_no;

-- 2.2 Per-pool stake distribution for specific epoch (SPDD reference data)
-- Use epoch_no = :epoch + 2 to get snapshot for :epoch
SELECT encode(ph.hash_raw, 'hex') AS pool_id,
       SUM(es.amount)::bigint     AS amount
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE es.epoch_no = :epoch + 2
GROUP BY ph.hash_raw
HAVING SUM(es.amount) > 0
ORDER BY ph.hash_raw;

-- 2.3 Find large stake changes between consecutive epochs (identifies re-delegations)
WITH epoch_n_stake AS (SELECT pool_id, SUM(amount) as stake_n
                       FROM epoch_stake
                       WHERE epoch_no = :epoch + 2
                       GROUP BY pool_id),
     epoch_n1_stake AS (SELECT pool_id, SUM(amount) as stake_n1
                        FROM epoch_stake
                        WHERE epoch_no = :epoch + 3
                        GROUP BY pool_id)
SELECT ph.view                                             as pool,
       en.stake_n,
       en1.stake_n1,
       COALESCE(en1.stake_n1, 0) - COALESCE(en.stake_n, 0) as difference
FROM epoch_n_stake en
         FULL OUTER JOIN epoch_n1_stake en1 ON en.pool_id = en1.pool_id
         LEFT JOIN pool_hash ph ON COALESCE(en.pool_id, en1.pool_id) = ph.id
WHERE ABS(COALESCE(en1.stake_n1, 0) - COALESCE(en.stake_n, 0)) > 1000000000000
ORDER BY ABS(COALESCE(en1.stake_n1, 0) - COALESCE(en.stake_n, 0)) DESC
LIMIT 20;

-- #############################################################################
-- SECTION 3: REWARDS ANALYSIS
-- Understanding reward distribution by type (leader, member, refund)
-- #############################################################################

-- 3.1 Total rewards by epoch and type
SELECT earned_epoch,
       type,
       SUM(amount) as total_amount,
       COUNT(*)    as count
FROM reward
WHERE earned_epoch BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY earned_epoch, type
ORDER BY earned_epoch, type;

-- 3.2 Rewards summary by epoch (total only)
SELECT earned_epoch,
       SUM(amount)             as total_rewards,
       COUNT(*)                as reward_count,
       COUNT(DISTINCT pool_id) as pools_with_rewards
FROM reward
WHERE earned_epoch BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY earned_epoch
ORDER BY earned_epoch;

-- 3.3 Leader rewards analysis
SELECT r.earned_epoch,
       COUNT(DISTINCT r.pool_id) as pools_with_leader_rewards,
       SUM(r.amount)             as total_leader_rewards
FROM reward r
WHERE r.type = 'leader'
  AND r.earned_epoch BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY r.earned_epoch
ORDER BY r.earned_epoch;

-- 3.4 Member rewards analysis (compare with SPDD delegator counts)
SELECT earned_epoch,
       COUNT(*)    as member_count,
       SUM(amount) as member_total
FROM reward
WHERE type = 'member'
  AND earned_epoch BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY earned_epoch
ORDER BY earned_epoch;

-- #############################################################################
-- SECTION 4: REGISTRATION & DEREGISTRATION
-- Critical for understanding addrsRew capture and stability window behavior
-- #############################################################################

-- 4.1 Count registrations/deregistrations by epoch
SELECT b.epoch_no,
       'registration' as event_type,
       COUNT(*)       as count
FROM stake_registration sr
         JOIN tx ON sr.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE b.epoch_no BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY b.epoch_no
UNION ALL
SELECT b.epoch_no,
       'deregistration' as event_type,
       COUNT(*)         as count
FROM stake_deregistration sd
         JOIN tx ON sd.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE b.epoch_no BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY b.epoch_no
ORDER BY epoch_no, event_type;

-- 4.2 Registration changes relative to stability window (4k/5 = 172800 slots)
-- Changes BEFORE stability window affect reward eligibility in Shelley-Babbage
SELECT b.epoch_no,
       CASE
           WHEN b.epoch_slot_no < 172800 THEN 'before_stability'
           WHEN b.epoch_slot_no = 172800 THEN 'at_stability'
           ELSE 'after_stability'
           END                                                                      as timing,
       CASE WHEN sr.tx_id IS NOT NULL THEN 'registration' ELSE 'deregistration' END as action,
       COUNT(*)                                                                     as count
FROM block b
         JOIN tx ON tx.block_id = b.id
         LEFT JOIN stake_registration sr ON sr.tx_id = tx.id
         LEFT JOIN stake_deregistration sd ON sd.tx_id = tx.id
WHERE b.epoch_no = :epoch
  AND (sr.tx_id IS NOT NULL OR sd.tx_id IS NOT NULL)
GROUP BY b.epoch_no,
         CASE
             WHEN b.epoch_slot_no < 172800 THEN 'before_stability'
             WHEN b.epoch_slot_no = 172800 THEN 'at_stability'
             ELSE 'after_stability'
             END,
         CASE WHEN sr.tx_id IS NOT NULL THEN 'registration' ELSE 'deregistration' END
ORDER BY timing, action;

-- 4.3 Deregistrations near stability window (detailed timing)
SELECT b.epoch_slot_no, COUNT(*) as deregistrations
FROM stake_deregistration sd
         JOIN tx ON sd.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE b.epoch_no = :epoch
  AND b.epoch_slot_no >= 172000
GROUP BY b.epoch_slot_no
ORDER BY b.epoch_slot_no;

-- 4.4 Find accounts that deregistered before stability window but got rewards
-- (indicates potential bug in reward filtering)
WITH deregistered_before_stability AS (SELECT sa.id as addr_id, sa.view
                                       FROM stake_deregistration sd
                                                JOIN tx ON sd.tx_id = tx.id
                                                JOIN block b ON tx.block_id = b.id
                                                JOIN stake_address sa ON sd.addr_id = sa.id
                                       WHERE b.epoch_no = :epoch
                                         AND b.epoch_slot_no < 172800)
SELECT r.earned_epoch,
       r.type,
       SUM(r.amount) as total_amount,
       COUNT(*)      as count
FROM reward r
         JOIN deregistered_before_stability d ON r.addr_id = d.addr_id
WHERE r.earned_epoch = :epoch - 2 -- Rewards earned 2 epochs before
GROUP BY r.earned_epoch, r.type;

-- #############################################################################
-- SECTION 5: POOL RETIREMENT & BLOCK PRODUCTION
-- Retired pools may still produce blocks due to slot leader schedule lag
-- #############################################################################

-- 5.1 Pools retiring around the epoch
SELECT pr.retiring_epoch,
       b.epoch_no as announced_epoch,
       ph.view    as pool
FROM pool_retire pr
         JOIN pool_hash ph ON pr.hash_id = ph.id
         JOIN tx ON pr.announced_tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE pr.retiring_epoch BETWEEN :epoch - 2 AND :epoch + 2
ORDER BY pr.retiring_epoch, b.epoch_no;

-- 5.2 Blocks per pool (to compare with rewards)
SELECT b.epoch_no,
       ph.view  as pool,
       COUNT(*) as blocks
FROM block b
         JOIN slot_leader sl ON b.slot_leader_id = sl.id
         JOIN pool_hash ph ON sl.pool_hash_id = ph.id
WHERE b.epoch_no = :epoch
  AND sl.pool_hash_id IS NOT NULL
GROUP BY b.epoch_no, ph.view
ORDER BY blocks DESC
LIMIT 50;

-- 5.3 Pools that produced blocks but have no leader rewards (or vice versa)
-- Indicates potential issue with block counting or reward calculation
WITH blocks_per_pool AS (SELECT sl.pool_hash_id, COUNT(*) as blocks
                         FROM block b
                                  JOIN slot_leader sl ON b.slot_leader_id = sl.id
                         WHERE b.epoch_no = :epoch
                           AND sl.pool_hash_id IS NOT NULL
                         GROUP BY sl.pool_hash_id),
     rewards_per_pool AS (SELECT pool_id, SUM(amount) as leader_rewards
                          FROM reward
                          WHERE earned_epoch = :epoch
                            AND type = 'leader'
                          GROUP BY pool_id)
SELECT ph.view                       as pool,
       COALESCE(b.blocks, 0)         as blocks,
       COALESCE(r.leader_rewards, 0) as leader_rewards
FROM pool_hash ph
         LEFT JOIN blocks_per_pool b ON b.pool_hash_id = ph.id
         LEFT JOIN rewards_per_pool r ON r.pool_id = ph.id
WHERE (b.blocks > 0 AND r.leader_rewards IS NULL)
   OR (b.blocks IS NULL AND r.leader_rewards > 0)
ORDER BY b.blocks DESC NULLS LAST
LIMIT 20;

-- #############################################################################
-- SECTION 6: PROTOCOL PARAMETERS
-- Important for understanding monetary expansion and fee calculations
-- #############################################################################

-- 6.1 Key protocol parameters across epochs
SELECT epoch_no,
       min_fee_a,
       min_fee_b,
       key_deposit,
       pool_deposit,
       monetary_expand_rate as rho,
       treasury_growth_rate as tau,
       optimal_pool_count,
       influence            as a0
FROM epoch_param
WHERE epoch_no BETWEEN :epoch - 2 AND :epoch + 2
ORDER BY epoch_no;

-- #############################################################################
-- SECTION 7: ERA BOUNDARY INVESTIGATION
-- Special queries for hard fork boundaries (Shelley→Allegra, etc.)
-- #############################################################################

-- 7.1 Check for MIRs (Move Instantaneous Rewards) - reserves/treasury transfers
SELECT earned_epoch,
       type,
       SUM(amount) as total,
       COUNT(*)    as count
FROM reward
WHERE type IN ('reserves', 'treasury')
  AND earned_epoch BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY earned_epoch, type
ORDER BY earned_epoch, type;

-- 7.2 reward_rest entries (includes MIRs and other non-pool rewards)
SELECT type,
       earned_epoch,
       spendable_epoch,
       SUM(amount) as total_amount,
       COUNT(*)    as count
FROM reward_rest
WHERE spendable_epoch BETWEEN :epoch - 2 AND :epoch + 2
GROUP BY type, earned_epoch, spendable_epoch
ORDER BY spendable_epoch, type;

-- =============================================================================
-- PARAMETERIZED POOL INVESTIGATION
-- Run with: psql -v epoch=235 -v pool="'pool1xxx...'"
-- =============================================================================

-- Pool stake across epochs
SELECT es.epoch_no - 2        as snapshot_epoch,
       SUM(es.amount)::bigint as total_stake,
       COUNT(*)               as num_delegators
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE ph.view = :pool
  AND es.epoch_no BETWEEN :epoch AND :epoch + 4
GROUP BY es.epoch_no
ORDER BY es.epoch_no;

-- =============================================================================
-- PARAMETERIZED STAKE ADDRESS INVESTIGATION
-- Run with: psql -v epoch=235 -v addr="'stake1uxxx...'"
-- =============================================================================

-- Stake address registration history
SELECT b.epoch_no,
       b.epoch_slot_no,
       'registration' as action
FROM stake_registration sr
         JOIN tx ON sr.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
         JOIN stake_address sa ON sr.addr_id = sa.id
WHERE sa.view = :addr
UNION ALL
SELECT b.epoch_no,
       b.epoch_slot_no,
       'deregistration' as action
FROM stake_deregistration sd
         JOIN tx ON sd.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
         JOIN stake_address sa ON sd.addr_id = sa.id
WHERE sa.view = :addr
ORDER BY epoch_no, epoch_slot_no;

-- =============================================================================
-- END OF DIAGNOSTIC QUERIES
-- =============================================================================
