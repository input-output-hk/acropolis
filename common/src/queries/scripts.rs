use crate::queries::errors::QueryError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ScriptsStateQuery {
    GetScriptsList,
    GetScriptInfo,
    GetScriptJSON,
    GetScriptCBOR,
    GetScriptRedeemers,
    GetScriptDatumJSON,
    GetScriptDatumCBOR,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ScriptsStateQueryResponse {
    ScriptsList(ScriptsList),
    ScriptInfo(ScriptInfo),
    ScriptJSON(ScriptJSON),
    ScriptCBOR(ScriptCBOR),
    ScriptRedeemers(ScriptRedeemers),
    ScriptDatumJSON(ScriptDatumJSON),
    ScriptDatumCBOR(ScriptDatumCBOR),
    Error(QueryError),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptsList {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptJSON {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptCBOR {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptRedeemers {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptDatumJSON {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScriptDatumCBOR {}
