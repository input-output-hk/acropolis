\copy (select * from gov_action_proposal left join tx on gov_action_proposal.tx_id = tx.id) to 'mysql_db_voting.csv' csv header
