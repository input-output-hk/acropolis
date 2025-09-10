use acropolis_common::{
    messages::{Message, RESTResponse, StateQuery, StateQueryResponse},
    queries::{
        assets::{AssetsStateQuery, AssetsStateQueryResponse},
        utils::query_state,
    },
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;

use crate::{handlers_config::HandlersConfig, types::AssetListEntryRest};

pub async fn handle_assets_list_blockfrost(
    context: Arc<Context<Message>>,
    _params: Vec<String>,
    handlers_config: Arc<HandlersConfig>,
) -> Result<RESTResponse> {
    let assets_list_msg = Arc::new(Message::StateQuery(StateQuery::Assets(
        AssetsStateQuery::GetAssetsList,
    )));
    let response = query_state(
        &context,
        &handlers_config.assets_query_topic,
        assets_list_msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Assets(
                AssetsStateQueryResponse::AssetsList(assets),
            )) => Ok(assets),
            _ => {
                return Err(anyhow::anyhow!(
                    "Unexpected response while retrieving assets list"
                ))
            }
        },
    )
    .await?;

    let rest: Vec<AssetListEntryRest> = response
        .into_iter()
        .map(|(policy_id, name, amount)| AssetListEntryRest {
            asset: format!("{}{}", hex::encode(policy_id), hex::encode(name.as_slice())),
            quantity: amount.to_string(),
        })
        .collect();

    match serde_json::to_string_pretty(&rest) {
        Ok(json) => Ok(RESTResponse::with_json(200, &json)),
        Err(e) => Ok(RESTResponse::with_text(
            500,
            &format!("Failed to serialize assets list: {e}"),
        )),
    }
}
