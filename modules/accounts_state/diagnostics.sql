-- 1. Check deregistrations at exactly slot 172800 in epoch 223
SELECT 'deregistration' as action, b.epoch_no, b.epoch_slot_no, sa.view
FROM stake_deregistration sd
         JOIN tx ON sd.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
         JOIN stake_address sa ON sd.addr_id = sa.id
WHERE b.epoch_no = 223
  AND b.epoch_slot_no = 172800;

-- 2. Check registrations/deregistrations NEAR the stability window (172800 ± 10 slots)
SELECT CASE WHEN sr.tx_id IS NOT NULL THEN 'registration' ELSE 'deregistration' END as action,
       b.epoch_no,
       b.epoch_slot_no,
       sa.view                                                                      as stake_address
FROM block b
         JOIN tx ON tx.block_id = b.id
         LEFT JOIN stake_registration sr ON sr.tx_id = tx.id
         LEFT JOIN stake_deregistration sd ON sd.tx_id = tx.id
         JOIN stake_address sa ON sa.id = COALESCE(sr.addr_id, sd.addr_id)
WHERE b.epoch_no = 223
  AND b.epoch_slot_no BETWEEN 172790 AND 172810
  AND (sr.tx_id IS NOT NULL OR sd.tx_id IS NOT NULL)
ORDER BY b.epoch_slot_no;

-- 3. Get the EXACT stake for pool1hcg8sa642... at epoch 223 from db-sync
SELECT SUM(es.amount) as total_stake,
       COUNT(*)       as delegator_count
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND es.epoch_no = 225;
-- epoch 223 snapshot

-- 4. List the TOP 10 delegators for that pool at epoch 223
SELECT sa.view as stake_address,
       es.amount
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
         JOIN stake_address sa ON es.addr_id = sa.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND es.epoch_no = 225
ORDER BY es.amount DESC
LIMIT 10;

SELECT sa.view as stake_address,
       r.amount,
       r.type
FROM reward r
         JOIN stake_address sa ON r.addr_id = sa.id
         JOIN pool_hash ph ON r.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND r.earned_epoch = 222
ORDER BY r.amount DESC
LIMIT 20;

-- 6. Total rewards for this pool's delegators in epoch 222
SELECT SUM(r.amount)             as total_rewards,
       COUNT(DISTINCT r.addr_id) as accounts_rewarded
FROM reward r
         JOIN pool_hash ph ON r.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND r.earned_epoch = 222;

-- 7. Check if there were any rewards to this pool that went to treasury
-- (instant_reward with type 'treasury' or rewards with spendable_epoch issues)
SELECT r.earned_epoch,
       r.spendable_epoch,
       r.type,
       SUM(r.amount) as total,
       COUNT(*)      as count
FROM reward r
         JOIN pool_hash ph ON r.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND r.earned_epoch BETWEEN 220 AND 224
GROUP BY r.earned_epoch, r.spendable_epoch, r.type
ORDER BY r.earned_epoch, r.type;


-- 8. Check cumulative rewards for the top delegators through epoch 222
SELECT sa.view       as stake_address,
       SUM(r.amount) as total_rewards_through_222
FROM reward r
         JOIN stake_address sa ON r.addr_id = sa.id
         JOIN epoch_stake es ON es.addr_id = sa.id AND es.epoch_no = 225
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND r.spendable_epoch <= 224 -- rewards paid by start of epoch 224
GROUP BY sa.view
ORDER BY total_rewards_through_222 DESC
LIMIT 10;

-- 9. Check if epoch 222 rewards exist for ANY pool (to confirm it's pool-specific)
SELECT COUNT(DISTINCT r.pool_id) as pools_with_rewards,
       SUM(r.amount)             as total_rewards,
       COUNT(*)                  as reward_count
FROM reward r
WHERE r.earned_epoch = 222;

-- 10. Check what epoch 222 rewards SHOULD look like for this pool
-- by looking at their stake in the epoch 220 snapshot (used for epoch 222 rewards)
SELECT SUM(es.amount) as stake_at_epoch_220,
       COUNT(*)       as delegator_count
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND es.epoch_no = 222;
-- This is the "go" snapshot for epoch 222 rewards

-- 11. Did this pool produce blocks in epoch 222?
SELECT COUNT(*) as blocks_produced
FROM block b
         JOIN slot_leader sl ON b.slot_leader_id = sl.id
         JOIN pool_hash ph ON sl.pool_hash_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND b.epoch_no = 222;

-- 12. Compare with a pool that DID get epoch 222 rewards
SELECT ph.view       as pool,
       SUM(r.amount) as epoch_222_rewards,
       COUNT(*)      as reward_count
FROM reward r
         JOIN pool_hash ph ON r.pool_id = ph.id
WHERE r.earned_epoch = 222
GROUP BY ph.view
ORDER BY epoch_222_rewards DESC
LIMIT 5;

-- 13. What's the TOTAL accumulated rewards for all delegators of this pool
-- that should be in the epoch 223 SPDD?
-- (Rewards spendable by epoch 224 = earned in epochs <= 222)
SELECT SUM(r.amount) as total_accumulated_rewards
FROM reward r
         JOIN epoch_stake es ON r.addr_id = es.addr_id AND es.epoch_no = 225
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND r.spendable_epoch <= 224;

-- 14. Break it down by earned_epoch to see the pattern
SELECT r.earned_epoch,
       r.spendable_epoch,
       SUM(r.amount)             as rewards,
       COUNT(DISTINCT r.addr_id) as accounts
FROM reward r
         JOIN epoch_stake es ON r.addr_id = es.addr_id AND es.epoch_no = 225
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND r.spendable_epoch <= 224
GROUP BY r.earned_epoch, r.spendable_epoch
ORDER BY r.earned_epoch;

-- 15. Check the pool's block production history around this time
SELECT b.epoch_no,
       COUNT(*) as blocks
FROM block b
         JOIN slot_leader sl ON b.slot_leader_id = sl.id
         JOIN pool_hash ph ON sl.pool_hash_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND b.epoch_no BETWEEN 218 AND 226
GROUP BY b.epoch_no
ORDER BY b.epoch_no;

-- 16. Critical: What is the EXACT expected SPDD value for this pool?
-- SPDD = SUM(utxo_stake + accumulated_rewards) for each delegator
-- Let's compute it properly
SELECT es.amount                                      as utxo_stake,
       COALESCE(rewards.total_rewards, 0)             as accumulated_rewards,
       es.amount + COALESCE(rewards.total_rewards, 0) as total_stake,
       sa.view                                        as stake_address
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
         JOIN stake_address sa ON es.addr_id = sa.id
         LEFT JOIN (SELECT r.addr_id, SUM(r.amount) as total_rewards
                    FROM reward r
                    WHERE r.spendable_epoch <= 224
                    GROUP BY r.addr_id) rewards ON rewards.addr_id = es.addr_id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND es.epoch_no = 225
ORDER BY total_stake DESC
LIMIT 15;

-- 17. Who received the epoch 222 reward for this pool?
SELECT sa.view as stake_address,
       r.amount,
       r.type,
       r.earned_epoch,
       r.spendable_epoch
FROM reward r
         JOIN stake_address sa ON r.addr_id = sa.id
         JOIN pool_hash ph ON r.pool_id = ph.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
  AND r.earned_epoch = 222;

-- 18. Is this the pool's reward account?
SELECT pu.reward_addr_id,
       sa.view as reward_account
FROM pool_update pu
         JOIN pool_hash ph ON pu.hash_id = ph.id
         JOIN stake_address sa ON pu.reward_addr_id = sa.id
WHERE ph.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
ORDER BY pu.registered_tx_id DESC
LIMIT 1;

-- 19. Is the reward account delegated to THIS pool at epoch 223?
SELECT es.epoch_no,
       es.amount,
       ph.view as delegated_to_pool
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
         JOIN stake_address sa ON es.addr_id = sa.id
WHERE sa.view = (SELECT sa2.view
                 FROM pool_update pu
                          JOIN pool_hash ph2 ON pu.hash_id = ph2.id
                          JOIN stake_address sa2 ON pu.reward_addr_id = sa2.id
                 WHERE ph2.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
                 ORDER BY pu.registered_tx_id DESC
                 LIMIT 1)
  AND es.epoch_no = 225;

-- 20. Find the actual reward record for earned_epoch=222 that goes to
-- a delegator of this pool at epoch 225
SELECT sa.view as stake_address,
       r.amount,
       r.type,
       r.earned_epoch,
       r.spendable_epoch,
       ph.view as reward_pool
FROM reward r
         JOIN stake_address sa ON r.addr_id = sa.id
         JOIN pool_hash ph ON r.pool_id = ph.id
WHERE r.earned_epoch = 222
  AND r.addr_id IN (SELECT es.addr_id
                    FROM epoch_stake es
                             JOIN pool_hash ph2 ON es.pool_id = ph2.id
                    WHERE ph2.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
                      AND es.epoch_no = 225);

-- 21. Check if the pool reward account received rewards from a DIFFERENT pool
SELECT r.earned_epoch,
       r.spendable_epoch,
       r.amount,
       r.type,
       ph.view as from_pool
FROM reward r
         JOIN pool_hash ph ON r.pool_id = ph.id
WHERE r.addr_id = 6101 -- The reward account ID
  AND r.spendable_epoch <= 224
ORDER BY r.earned_epoch;

-- 22. Let's see ALL rewards with earned_epoch=222 for accounts
-- that are delegated to our pool at epoch 225, regardless of source pool
SELECT sa.view           as stake_address,
       r.amount,
       r.type,
       ph_reward.view    as reward_from_pool,
       ph_delegated.view as delegated_to_at_225
FROM reward r
         JOIN stake_address sa ON r.addr_id = sa.id
         JOIN pool_hash ph_reward ON r.pool_id = ph_reward.id
         JOIN epoch_stake es ON es.addr_id = r.addr_id AND es.epoch_no = 225
         JOIN pool_hash ph_delegated ON es.pool_id = ph_delegated.id
WHERE r.earned_epoch = 222
  AND ph_delegated.view = 'pool1hcg8sa642l0xeygkzvpgn3sfj5s2yeuzpws7a0gypyy7grrjcje'
ORDER BY r.amount DESC;

-- 23. Check registration history for the account that's missing rewards
SELECT 'registration' as action,
       b.epoch_no,
       b.epoch_slot_no,
       b.slot_no
FROM stake_registration sr
         JOIN stake_address sa ON sr.addr_id = sa.id
         JOIN tx ON sr.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE sa.view = 'stake1uyx4e4v2dqxsacw7dtmh4fmesehs48sr3qhu5gxnqns8t4cw0src5'
ORDER BY b.slot_no;

-- 24. Check deregistration history
SELECT 'deregistration' as action,
       b.epoch_no,
       b.epoch_slot_no,
       b.slot_no
FROM stake_deregistration sd
         JOIN stake_address sa ON sd.addr_id = sa.id
         JOIN tx ON sd.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE sa.view = 'stake1uyx4e4v2dqxsacw7dtmh4fmesehs48sr3qhu5gxnqns8t4cw0src5'
ORDER BY b.slot_no;

-- 25. Check delegation history - when did they move from pool104fdj... topool1hcg8sa...?
SELECT b.epoch_no,
       b.epoch_slot_no,
       ph.view as delegated_to_pool
FROM delegation d
         JOIN stake_address sa ON d.addr_id = sa.id
         JOIN pool_hash ph ON d.pool_hash_id = ph.id
         JOIN tx ON d.tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE sa.view = 'stake1uyx4e4v2dqxsacw7dtmh4fmesehs48sr3qhu5gxnqns8t4cw0src5'
ORDER BY b.slot_no;


-- 26. Did pool104fdj0xdhr8... produce blocks in epoch 222?
SELECT b.epoch_no,
       COUNT(*) as blocks
FROM block b
         JOIN slot_leader sl ON b.slot_leader_id = sl.id
         JOIN pool_hash ph ON sl.pool_hash_id = ph.id
WHERE ph.view = 'pool104fdj0xdhr8cqedwn0lf8dk206ryn62kn8mtyym020nuxthjgvj'
  AND b.epoch_no BETWEEN 220 AND 224
GROUP BY b.epoch_no
ORDER BY b.epoch_no;

-- 27. Verify this pool exists in staking snapshot for epoch 220
SELECT COUNT(*)       as delegator_count,
       SUM(es.amount) as total_stake
FROM epoch_stake es
         JOIN pool_hash ph ON es.pool_id = ph.id
WHERE ph.view = 'pool104fdj0xdhr8cqedwn0lf8dk206ryn62kn8mtyym020nuxthjgvj'
  AND es.epoch_no = 222;
-- "go" snapshot for epoch 222 rewards

-- 28. Check if pool104fdj0xdhr8... retired or had any updates around epoch 222
SELECT 'update'   as action,
       pu.active_epoch_no,
       b.epoch_no as tx_epoch
FROM pool_update pu
         JOIN pool_hash ph ON pu.hash_id = ph.id
         JOIN tx ON pu.registered_tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE ph.view = 'pool104fdj0xdhr8cqedwn0lf8dk206ryn62kn8mtyym020nuxthjgvj'
ORDER BY b.slot_no;

-- 29. Check if pool104fdj0xdhr8... had any retirement announcements
SELECT 'retire'   as action,
       pr.retiring_epoch,
       b.epoch_no as announced_epoch
FROM pool_retire pr
         JOIN pool_hash ph ON pr.hash_id = ph.id
         JOIN tx ON pr.announced_tx_id = tx.id
         JOIN block b ON tx.block_id = b.id
WHERE ph.view = 'pool104fdj0xdhr8cqedwn0lf8dk206ryn62kn8mtyym020nuxthjgvj'
ORDER BY b.slot_no;